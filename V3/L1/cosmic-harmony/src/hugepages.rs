//! HugePages memory allocator for mining scratchpads.
//!
//! Inspired by XMRig's VirtualMemory — uses OS-level huge pages (2 MiB) to
//! eliminate TLB misses during memory-hard scratchpad operations.
//!
//! The 64 KiB Ekam Deeksha scratchpad has 1024 pseudo-random 64-byte block
//! accesses per hash. With standard 4 KiB pages, each access can cause a TLB
//! miss. With 2 MiB huge pages, the entire scratchpad fits in ONE TLB entry.
//!
//! Platform support:
//! - macOS (arm64/x86_64): VM_FLAGS_SUPERPAGE_SIZE_2MB via mmap
//! - Linux: MAP_HUGETLB | MAP_POPULATE via mmap
//! - Windows: VirtualAlloc with MEM_LARGE_PAGES (requires SeLockMemoryPrivilege)
//! - Fallback: aligned standard allocation via mmap MAP_ANONYMOUS
//!
//! Usage:
//! ```ignore
//! let hp = HugePageScratchpad::new(64 * 1024).unwrap();
//! let buf: &mut [u8] = hp.as_mut_slice();
//! // ... use buf as 64 KiB scratchpad ...
//! ```

use std::ptr;

/// Size of a "huge page" for allocation alignment (2 MiB).
const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// A single scratchpad buffer backed by huge pages (when available).
pub struct HugePageScratchpad {
    ptr: *mut u8,
    /// Actual mapped size (rounded to huge page boundary).
    mapped_size: usize,
    /// Logical scratchpad size (e.g. 64 KiB).
    logical_size: usize,
    /// Whether huge pages were successfully obtained.
    huge_pages: bool,
    /// Whether the memory is locked.
    locked: bool,
}

// SAFETY: The buffer is exclusively owned by this struct (no aliasing).
unsafe impl Send for HugePageScratchpad {}
unsafe impl Sync for HugePageScratchpad {}

/// Result of huge page availability check.
#[derive(Debug, Clone)]
pub struct HugePagesInfo {
    pub available: bool,
    pub allocated: bool,
    pub page_size: usize,
}

impl HugePageScratchpad {
    /// Allocate a scratchpad buffer, preferring huge pages.
    ///
    /// Falls back to regular mmap if huge pages are unavailable.
    /// The buffer is zero-initialized and memory-locked.
    pub fn new(size: usize) -> Result<Self, String> {
        let mapped_size = align_to_huge_page(size);

        // Try huge pages first, then fall back to regular mmap
        let (ptr, huge_pages) = alloc_huge_pages(mapped_size).unwrap_or_else(|| {
            let p = alloc_regular(mapped_size);
            // Try transparent huge pages as a middle ground
            if let Some(p) = p {
                advise_huge_pages(p, mapped_size);
            }
            (p, false)
        });

        let ptr =
            ptr.ok_or_else(|| format!("Failed to allocate {} KiB scratchpad memory", size / 1024))?;

        // Lock memory to prevent swapping (best-effort)
        let locked = mlock(ptr, mapped_size);

        // Advise kernel about random access pattern
        madvise_random(ptr, mapped_size);

        Ok(HugePageScratchpad {
            ptr,
            mapped_size,
            logical_size: size,
            huge_pages,
            locked,
        })
    }

    /// Returns a mutable slice to the scratchpad buffer.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.logical_size) }
    }

    /// Returns an immutable slice to the scratchpad buffer.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.logical_size) }
    }

    /// Returns a raw mutable pointer.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    /// Whether this allocation is backed by huge pages.
    #[inline]
    pub fn is_huge_pages(&self) -> bool {
        self.huge_pages
    }

    /// Whether this allocation is memory-locked.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Logical scratchpad size.
    #[inline]
    pub fn len(&self) -> usize {
        self.logical_size
    }

    /// Whether this scratchpad has zero logical size.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.logical_size == 0
    }
}

impl Drop for HugePageScratchpad {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            #[cfg(unix)]
            {
                if self.locked {
                    unsafe { libc::munlock(self.ptr as *const libc::c_void, self.mapped_size) };
                }
                unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.mapped_size) };
            }
            #[cfg(not(unix))]
            {
                #[cfg(target_os = "windows")]
                {
                    #[link(name = "kernel32")]
                    extern "system" {
                        fn VirtualUnlock(lp_address: *mut std::ffi::c_void, dw_size: usize) -> i32;
                        fn VirtualFree(
                            lp_address: *mut std::ffi::c_void,
                            dw_size: usize,
                            dw_free_type: u32,
                        ) -> i32;
                    }
                    const MEM_RELEASE: u32 = 0x8000;
                    unsafe {
                        if self.locked {
                            VirtualUnlock(self.ptr as *mut std::ffi::c_void, self.mapped_size);
                        }
                        if self.huge_pages {
                            VirtualFree(self.ptr as *mut std::ffi::c_void, 0, MEM_RELEASE);
                        } else {
                            use std::alloc::{dealloc, Layout};
                            if let Ok(layout) =
                                Layout::from_size_align(self.mapped_size, HUGE_PAGE_SIZE)
                            {
                                dealloc(self.ptr, layout);
                            }
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    use std::alloc::{dealloc, Layout};
                    if let Ok(layout) = Layout::from_size_align(self.mapped_size, HUGE_PAGE_SIZE) {
                        unsafe { dealloc(self.ptr, layout) };
                    }
                }
            }
            self.ptr = ptr::null_mut();
        }
    }
}

/// Check if huge pages are available on this system.
pub fn is_huge_pages_available() -> HugePagesInfo {
    #[cfg(target_os = "macos")]
    {
        // macOS arm64 (Apple Silicon) natively uses 16K pages.
        // VM_FLAGS_SUPERPAGE_SIZE_2MB is x86_64-only on macOS.
        // On arm64, we fall back to regular mmap with madvise hints.
        // Even without superpages, 16K native pages mean the 64 KiB
        // scratchpad only needs 4 TLB entries (vs 16 on x86_64 4K pages).
        #[cfg(target_arch = "aarch64")]
        let available = false; // superpages not supported on arm64 macOS
        #[cfg(not(target_arch = "aarch64"))]
        let available = true; // x86_64 macOS supports superpages

        HugePagesInfo {
            available,
            allocated: false,
            page_size: if available { HUGE_PAGE_SIZE } else { 16384 }, // native 16K on arm64
        }
    }

    #[cfg(target_os = "linux")]
    {
        let available = std::fs::read_to_string("/proc/sys/vm/nr_hugepages")
            .map(|s| s.trim().parse::<u64>().unwrap_or(0) > 0)
            .unwrap_or(false);

        HugePagesInfo {
            available,
            allocated: false,
            page_size: HUGE_PAGE_SIZE,
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        #[cfg(target_os = "windows")]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetLargePageMinimum() -> usize;
            }
            let large_min = unsafe { GetLargePageMinimum() };
            let available = large_min > 0;
            HugePagesInfo {
                available,
                allocated: false,
                page_size: if available { large_min } else { 4096 },
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            HugePagesInfo {
                available: false,
                allocated: false,
                page_size: 4096,
            }
        }
    }
}

// ============================================================================
// Platform-specific allocation
// ============================================================================

/// Try to allocate memory backed by huge pages.
/// Returns (pointer, true) on success, or None on failure.
fn alloc_huge_pages(size: usize) -> Option<(Option<*mut u8>, bool)> {
    let ptr = alloc_huge_pages_inner(size);
    if let Some(p) = ptr {
        if !p.is_null() {
            return Some((Some(p), true));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn alloc_huge_pages_inner(size: usize) -> Option<*mut u8> {
    // macOS: VM_FLAGS_SUPERPAGE_SIZE_2MB = 0x40000
    // This is the same flag XMRig uses for macOS huge page allocation.
    const VM_FLAGS_SUPERPAGE_SIZE_2MB: i32 = 0x40000;

    let ptr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            VM_FLAGS_SUPERPAGE_SIZE_2MB, // fd field doubles as vm_flags on macOS
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        None
    } else {
        Some(ptr as *mut u8)
    }
}

#[cfg(target_os = "linux")]
fn alloc_huge_pages_inner(size: usize) -> Option<*mut u8> {
    let ptr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_HUGETLB | libc::MAP_POPULATE,
            -1,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        None
    } else {
        Some(ptr as *mut u8)
    }
}

#[cfg(target_os = "windows")]
fn alloc_huge_pages_inner(size: usize) -> Option<*mut u8> {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(
            lp_address: *mut c_void,
            dw_size: usize,
            fl_allocation_type: u32,
            fl_protect: u32,
        ) -> *mut c_void;
        fn GetLargePageMinimum() -> usize;
        fn GetCurrentProcess() -> isize;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(
            process_handle: isize,
            desired_access: u32,
            token_handle: *mut isize,
        ) -> i32;
        fn LookupPrivilegeValueA(
            lp_system_name: *const u8,
            lp_name: *const u8,
            lp_luid: *mut u64,
        ) -> i32;
        fn AdjustTokenPrivileges(
            token_handle: isize,
            disable_all: i32,
            new_state: *const u8,
            buffer_length: u32,
            previous_state: *mut u8,
            return_length: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const MEM_LARGE_PAGES: u32 = 0x20000000;
    const PAGE_READWRITE: u32 = 0x04;
    const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
    const TOKEN_QUERY: u32 = 0x0008;
    const SE_PRIVILEGE_ENABLED: u32 = 0x00000002;

    unsafe {
        // Try to enable SeLockMemoryPrivilege
        let mut token: isize = 0;
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) != 0
        {
            let mut luid: u64 = 0;
            if LookupPrivilegeValueA(
                ptr::null(),
                c"SeLockMemoryPrivilege".as_ptr() as *const u8,
                &mut luid,
            ) != 0
            {
                // TOKEN_PRIVILEGES struct: count(u32) + padding(u32) + LUID(u64) + Attributes(u32)
                #[repr(C)]
                struct TokenPrivileges {
                    count: u32,
                    _pad: u32,
                    luid: u64,
                    attributes: u32,
                }
                let tp = TokenPrivileges {
                    count: 1,
                    _pad: 0,
                    luid,
                    attributes: SE_PRIVILEGE_ENABLED,
                };
                AdjustTokenPrivileges(
                    token,
                    0,
                    &tp as *const _ as *const u8,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            CloseHandle(token);
        }

        let large_min = GetLargePageMinimum();
        if large_min == 0 {
            return None;
        }
        // Round size up to large page boundary
        let alloc_size = (size + large_min - 1) & !(large_min - 1);

        let ptr = VirtualAlloc(
            ptr::null_mut(),
            alloc_size,
            MEM_COMMIT | MEM_RESERVE | MEM_LARGE_PAGES,
            PAGE_READWRITE,
        );

        if ptr.is_null() {
            None
        } else {
            // Zero-initialize (VirtualAlloc guarantees zeroed pages)
            Some(ptr as *mut u8)
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn alloc_huge_pages_inner(_size: usize) -> Option<*mut u8> {
    None
}

/// Allocate regular mmap memory (fallback when huge pages unavailable).
#[cfg(unix)]
fn alloc_regular(size: usize) -> Option<*mut u8> {
    let ptr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        None
    } else {
        Some(ptr as *mut u8)
    }
}

/// Windows fallback: aligned allocation via std::alloc.
#[cfg(not(unix))]
fn alloc_regular(size: usize) -> Option<*mut u8> {
    use std::alloc::{alloc_zeroed, Layout};
    let layout = Layout::from_size_align(size, HUGE_PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc_zeroed(layout) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

/// Try to enable transparent huge pages for a memory region (Linux only).
fn advise_huge_pages(ptr: *mut u8, size: usize) {
    #[cfg(target_os = "linux")]
    unsafe {
        // MADV_HUGEPAGE = 14
        libc::madvise(ptr as *mut libc::c_void, size, libc::MADV_HUGEPAGE);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ptr, size);
    }
}

/// Lock memory to prevent swapping (best-effort).
#[cfg(unix)]
fn mlock(ptr: *mut u8, size: usize) -> bool {
    unsafe { libc::mlock(ptr as *const libc::c_void, size) == 0 }
}

#[cfg(not(unix))]
fn mlock(ptr: *mut u8, size: usize) -> bool {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn VirtualLock(lp_address: *mut std::ffi::c_void, dw_size: usize) -> i32;
        }
        unsafe { VirtualLock(ptr as *mut std::ffi::c_void, size) != 0 }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (ptr, size);
        false
    }
}

/// Advise the kernel that access will be random.
fn madvise_random(ptr: *mut u8, size: usize) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    unsafe {
        libc::madvise(
            ptr as *mut libc::c_void,
            size,
            libc::MADV_RANDOM | libc::MADV_WILLNEED,
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (ptr, size);
    }
}

/// Align size up to huge page boundary.
fn align_to_huge_page(size: usize) -> usize {
    (size + HUGE_PAGE_SIZE - 1) & !(HUGE_PAGE_SIZE - 1)
}

// ============================================================================
// Thread-local pool — one HugePageScratchpad per mining thread
// ============================================================================

use std::cell::RefCell;

// Thread-local huge-page-backed scratchpad buffer (one per mining thread).
thread_local! {
    static HP_SCRATCHPAD: RefCell<Option<HugePageScratchpad>> = const { RefCell::new(None) };
}

/// Execute a closure with the thread-local huge-page scratchpad.
///
/// On first call per thread, allocates a new HugePageScratchpad.
/// Subsequent calls reuse the same buffer (zero-cost after init).
#[inline]
pub fn with_huge_page_scratchpad<F, R>(size: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    HP_SCRATCHPAD.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() || opt.as_ref().map(|hp| hp.len()) != Some(size) {
            match HugePageScratchpad::new(size) {
                Ok(hp) => {
                    let status = if hp.is_huge_pages() {
                        "HUGE PAGES"
                    } else {
                        "regular pages"
                    };
                    let lock = if hp.is_locked() { "+locked" } else { "" };
                    log::info!(
                        "Scratchpad allocated: {} KiB on {} {}",
                        size / 1024,
                        status,
                        lock
                    );
                    *opt = Some(hp);
                }
                Err(e) => {
                    // HugePageScratchpad::new already falls back from huge
                    // pages to regular mmap internally. Reaching this branch
                    // means even regular mmap failed — i.e. the process is
                    // genuinely out of address space / memory. The node
                    // cannot verify PoW without a scratchpad, so abort
                    // loudly rather than silently skip verification.
                    log::error!(
                        "Scratchpad allocation of {} KiB failed ({}); cannot \
                         participate in PoW verification without it.",
                        size / 1024,
                        e
                    );
                    panic!("Cannot allocate scratchpad: {}", e);
                }
            }
        }
        let hp = opt.as_mut().unwrap();
        f(hp.as_mut_slice())
    })
}

/// Check if the current thread's scratchpad is using huge pages.
pub fn current_thread_has_huge_pages() -> bool {
    HP_SCRATCHPAD.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|hp| hp.is_huge_pages())
            .unwrap_or(false)
    })
}

/// Get a human-readable memory status line for the miner banner.
///
/// Example output:
/// - "HUGE PAGES 2048 KiB + mlock (64 KiB scratchpad)"
/// - "mmap 16K pages + mlock (64 KiB scratchpad)"
/// - "mmap regular (64 KiB scratchpad)"
pub fn memory_status_line(scratchpad_size: usize) -> String {
    let info = is_huge_pages_available();

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let platform_note = "Apple Silicon 16K native pages";
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    let platform_note = "macOS x86_64 superpages";
    #[cfg(target_os = "linux")]
    let platform_note = if info.available {
        "Linux HugePages enabled"
    } else {
        "Linux (enable hugepages: sysctl vm.nr_hugepages=128)"
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let platform_note = if cfg!(target_os = "windows") {
        if info.available {
            "Windows Large Pages (VirtualAlloc)"
        } else {
            "Windows (enable: secpol.msc → Lock pages in memory)"
        }
    } else {
        "standard pages"
    };

    format!(
        "{} KiB scratchpad | {} KiB pages | {} | {}",
        scratchpad_size / 1024,
        info.page_size / 1024,
        if info.available {
            "HUGEPAGES ready"
        } else {
            "mmap fallback"
        },
        platform_note,
    )
}
