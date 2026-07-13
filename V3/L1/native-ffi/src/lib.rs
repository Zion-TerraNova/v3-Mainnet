//! # zion-native-ffi
//!
//! Feature-gated safe Rust wrappers around the ZION native C algorithm libraries.
//!
//! ## Feature flags
//!
//! Enable algorithms individually or all at once:
//!
//! ```toml
//! # In Cargo.toml of your crate:
//! zion-native-ffi = { path = "../native-ffi", features = ["native-all"] }
//! ```
//!
//! | Feature                | Algorithm       | Coins           |
//! |------------------------|-----------------|-----------------|
//! | `native-etchash`       | Ethash/EtcHash  | ETC             |
//! | `native-kawpow`        | KawPow          | RVN, CLORE      |
//! | `native-autolykos`     | Autolykos v2    | ERG             |
//! | `native-kheavyhash`    | kHeavyHash      | KAS             |
//! | `native-blake3-algo`   | Blake3          | ALPH, DCR       |
//! | `native-cosmic-harmony`| Cosmic Harmony v3 | ZION          |
//! | `native-verushash`     | VerusHash v2.2  | VRSC            |
//! | `native-randomx`       | RandomX         | XMR, ZEPH       |
//! | `native-all`           | All of the above| —               |
//!
//! ## Safety contracts
//!
//! Every `unsafe extern "C"` block in this crate documents — at the
//! function-declaration level — the preconditions the C side relies on
//! (pointer non-null, valid for read/write of N bytes, proper alignment,
//! buffer non-aliasing, thread-safety, lifetime of returned pointers).
//!
//! The "infallible" safe wrappers (`hash`, `mine`, `verify`, …) preserve the
//! historical 2.9.x API and uphold those contracts internally. They only
//! `unwrap()` invariants that hold trivially for a `&[u8]` Rust slice (whose
//! data pointer is non-null and aligned for `u8`) and for stack-allocated
//! `[u8; 32]` outputs.
//!
//! For new call sites that take **untrusted-length** inputs (e.g. RPC,
//! deserialised network frames, JSON payloads), prefer the fallible
//! [`try_*`](safety) wrappers — they reject empty, oversized, or aliasing
//! inputs at the Rust boundary instead of trapping deep inside the C code.
//!
//! ## Version strings
//!
//! C entry points named `*_version()` return a static `*const c_char` pointing
//! into read-only program memory. Callers **must not** `free()` the returned
//! pointer. Use [`safety::read_c_version_string`] to convert it into an owned
//! `Option<String>` (None on null, lossy-UTF-8 decode otherwise).

// ---------------------------------------------------------------------------
// Shared safety helpers — used by every algorithm module's `try_*` wrappers.
// ---------------------------------------------------------------------------

pub mod safety {
    //! Shared FFI safety primitives.
    //!
    //! Defines [`FfiError`] (a typed error returned by every fallible safe
    //! wrapper), input-length bounds, and helpers to convert C-owned
    //! NUL-terminated `*const c_char` version strings into owned `String`s
    //! without trusting the C side to bound the length.

    use std::ffi::CStr;
    use std::fmt;

    /// Maximum byte length accepted by any safe FFI wrapper for a header /
    /// input slice.
    ///
    /// Algorithms accept far smaller payloads in practice (typically 32–80
    /// bytes for block headers). This 1 MiB ceiling exists to defensively
    /// reject obviously-malformed inputs from untrusted sources before they
    /// reach C — it is not a per-algorithm correctness bound.
    pub const MAX_INPUT_LEN_BYTES: usize = 1 << 20; // 1 MiB

    /// Maximum NUL-terminated length we are willing to scan when converting a
    /// C version string to an owned `String`. A correctly-implemented
    /// `*_version()` returns a pointer into a small static literal; this cap
    /// bounds runaway scans if the C side ever returns an unterminated buffer.
    pub const MAX_C_STRING_SCAN_BYTES: usize = 4096;

    /// Errors returned by fallible FFI wrappers.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FfiError {
        /// Caller passed a zero-length input slice. Safe wrappers reject this
        /// because some C entry points dereference `input[0]` unconditionally.
        EmptyInput,
        /// Input length exceeds [`MAX_INPUT_LEN_BYTES`].
        InputTooLarge { len: usize, max: usize },
        /// `*_version()` returned a null pointer.
        NullVersionString,
        /// `*_version()` returned a pointer that did not terminate within
        /// [`MAX_C_STRING_SCAN_BYTES`] — treated as a faulty C library.
        UnterminatedVersionString { scanned: usize },
        /// A C entry point that should be 0/1-valued returned an out-of-band
        /// value (e.g. a negative error code from a future ABI revision).
        UnexpectedReturnCode { c_function: &'static str, code: i32 },
    }

    impl fmt::Display for FfiError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                FfiError::EmptyInput => f.write_str("ffi_input_empty"),
                FfiError::InputTooLarge { len, max } => {
                    write!(f, "ffi_input_too_large: len={len}, max={max}")
                }
                FfiError::NullVersionString => f.write_str("ffi_version_null_pointer"),
                FfiError::UnterminatedVersionString { scanned } => {
                    write!(f, "ffi_version_unterminated: scanned={scanned} bytes")
                }
                FfiError::UnexpectedReturnCode { c_function, code } => {
                    write!(f, "ffi_unexpected_return_code: {c_function}={code}")
                }
            }
        }
    }

    impl std::error::Error for FfiError {}

    /// Validate that `input` is non-empty and at most [`MAX_INPUT_LEN_BYTES`].
    ///
    /// Returns `Ok(())` on success. This is the single guard that **every**
    /// `try_*` wrapper applies before touching C.
    pub fn validate_input_len(input: &[u8]) -> Result<(), FfiError> {
        if input.is_empty() {
            return Err(FfiError::EmptyInput);
        }
        if input.len() > MAX_INPUT_LEN_BYTES {
            return Err(FfiError::InputTooLarge {
                len: input.len(),
                max: MAX_INPUT_LEN_BYTES,
            });
        }
        Ok(())
    }

    /// Convert a C-owned static `*const c_char` returned from `*_version()`
    /// into an owned, lossy-UTF-8-decoded `String`.
    ///
    /// Contract on the caller:
    ///
    /// - `ptr` must either be null, or point to a NUL-terminated byte
    ///   sequence in valid memory that the C library owns and keeps alive
    ///   for the entire process lifetime (typical for `static const char*`
    ///   string literals).
    /// - The pointer must **not** be freed by the caller.
    ///
    /// We additionally bound the scan with a strlen-equivalent cap of
    /// [`MAX_C_STRING_SCAN_BYTES`] so a buggy C library that returns an
    /// unterminated buffer cannot make this function read arbitrary memory.
    ///
    /// # Safety
    ///
    /// `ptr` must satisfy the C-side preconditions described above. Passing a
    /// dangling, aliased-mutably, or freed pointer is undefined behaviour.
    pub unsafe fn read_c_version_string(ptr: *const std::ffi::c_char) -> Result<String, FfiError> {
        if ptr.is_null() {
            return Err(FfiError::NullVersionString);
        }

        // strnlen-equivalent: find the NUL terminator without reading past
        // MAX_C_STRING_SCAN_BYTES, even if the C side is buggy.
        let mut len = 0usize;
        while len < MAX_C_STRING_SCAN_BYTES {
            // SAFETY: The pointer is non-null; the caller guarantees it is
            // valid for reads of a NUL-terminated sequence. We bound the scan
            // so we never read past the cap regardless of whether the
            // terminator exists.
            let byte = unsafe { *ptr.add(len) };
            if byte == 0 {
                break;
            }
            len += 1;
        }

        if len == MAX_C_STRING_SCAN_BYTES {
            return Err(FfiError::UnterminatedVersionString {
                scanned: MAX_C_STRING_SCAN_BYTES,
            });
        }

        // SAFETY: We have located a NUL terminator within the cap, so
        // [ptr, ptr + len + 1) is a valid C string by construction.
        let cstr = unsafe { CStr::from_ptr(ptr) };
        Ok(cstr.to_string_lossy().into_owned())
    }

    /// Map a C `int32_t` `0`/`1`-valued boolean return into a strict `bool`,
    /// flagging any other value as an FFI ABI break instead of silently
    /// reading it as `false`.
    pub fn parse_c_bool(c_function: &'static str, code: i32) -> Result<bool, FfiError> {
        match code {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(FfiError::UnexpectedReturnCode { c_function, code }),
        }
    }
}

// ---------------------------------------------------------------------------
// Etchash / Ethash
// ---------------------------------------------------------------------------

#[cfg(feature = "native-etchash")]
pub mod etchash {
    //! # Safety / threading model
    //!
    //! - **Not thread-safe.** Maintains a global per-epoch DAG cache; concurrent
    //!   calls into [`hash`] / [`verify`] / [`init`] from multiple threads are
    //!   undefined behaviour. Wrap in `Mutex` or pin to a single executor.
    //! - [`init`] is idempotent at the C level but must complete before any
    //!   `hash` / `verify` call. The infallible [`hash`]/[`verify`] wrappers
    //!   do **not** call init; callers are responsible for invoking it once
    //!   at startup.
    //! - The pointer returned by [`ethash_version`] points into static program
    //!   memory and must not be freed.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Initialise the global Ethash epoch-cache state for epoch 0.
        ///
        /// # Safety
        /// Must be called from a single thread before any other entry point
        /// in this module. Idempotent.
        pub fn ethash_init();

        /// Compute the Ethash/EtcHash mix-hash of (header, nonce, height).
        ///
        /// # Safety
        /// - `header` must point to at least `header_len` initialised bytes
        ///   readable for the duration of the call. `header_len` may be 0 if
        ///   `header` is non-null and points to valid memory; the C side
        ///   internally zero-pads to 32 bytes.
        /// - `output` must point to a writable region of at least 32 bytes,
        ///   non-aliasing with `header`.
        /// - Caller must hold exclusive access to the global epoch cache (see
        ///   module-level threading note).
        pub fn ethash_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            output: *mut u8,
        );

        /// Verify that the mix-hash for (header, nonce, height) is below
        /// `target` (32-byte LE big-int).
        ///
        /// # Safety
        /// Same pointer-validity / threading rules as [`ethash_hash`], plus
        /// `target` must point to 32 readable bytes.
        ///
        /// # Returns
        /// `1` for a valid solution, `0` otherwise. Other values indicate an
        /// ABI break and should be surfaced as [`FfiError::UnexpectedReturnCode`].
        pub fn ethash_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            target: *const u8,
        ) -> i32;

        /// Pure function — derive the epoch from a block number. Safe to call
        /// from any thread without prior `ethash_init`.
        pub fn ethash_get_epoch(block_number: u32) -> u32;

        /// Run an internal microbenchmark and return hashes/second. Honours
        /// the same threading rules as [`ethash_hash`].
        pub fn ethash_benchmark(iterations: i32) -> f64;

        /// Free the global epoch-cache. Optional — only meaningful for
        /// shutdown paths in long-lived processes.
        pub fn ethash_cleanup();

        /// Return a `'static` pointer to the linked C library's version
        /// literal (read-only, must not be freed). May be null on stub builds.
        pub fn ethash_version() -> *const std::ffi::c_char;
    }

    /// Compute the Ethash/EtcHash of a block header.
    ///
    /// Returns 32-byte mix hash. Wrapper for back-compat; uses the underlying
    /// C function with the caller's slice. Empty-slice inputs are forwarded
    /// to C unchanged to preserve historical behaviour — prefer
    /// [`try_hash`] when the slice originates from untrusted code.
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: `header.as_ptr()` is non-null and valid for `header.len()`
        // bytes; `out.as_mut_ptr()` is valid for 32 writable bytes; the two
        // regions cannot alias because `out` is fresh stack memory. Threading
        // contract is the caller's responsibility (see module docs).
        unsafe {
            ethash_hash(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                out.as_mut_ptr(),
            );
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    ///
    /// See [`safety::FfiError`] for the failure modes.
    pub fn try_hash(header: &[u8], nonce: u64, height: u32) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        Ok(hash(header, nonce, height))
    }

    /// Returns `true` if the computed hash is below `target` (LE big-int comparison).
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: &[u8; 32]) -> bool {
        // SAFETY: same invariants as `hash`; `target` is a `&[u8; 32]` so
        // `as_ptr()` is non-null and valid for 32 readable bytes. We treat
        // any non-1 return as `false` to preserve the historical API.
        unsafe {
            ethash_verify(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                target.as_ptr(),
            ) == 1
        }
    }

    /// Strict variant of [`verify`] that rejects empty / oversized inputs and
    /// surfaces unexpected C return codes as [`FfiError::UnexpectedReturnCode`]
    /// instead of silently coercing them to `false`.
    pub fn try_verify(
        header: &[u8],
        nonce: u64,
        height: u32,
        target: &[u8; 32],
    ) -> Result<bool, FfiError> {
        safety::validate_input_len(header)?;
        // SAFETY: see `verify`.
        let code = unsafe {
            ethash_verify(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                target.as_ptr(),
            )
        };
        safety::parse_c_bool("ethash_verify", code)
    }

    /// Run a quick initialisation for epoch 0 (safe to call multiple times).
    pub fn init() {
        // SAFETY: `ethash_init` has no inputs and is idempotent at the C
        // level; thread-safety is the caller's responsibility.
        unsafe {
            ethash_init();
        }
    }

    /// Hash/s estimate over `iterations` single-hash invocations.
    pub fn benchmark(iterations: i32) -> f64 {
        // SAFETY: `ethash_benchmark` allocates internally and respects the
        // caller-side threading contract documented at module level.
        unsafe { ethash_benchmark(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `ethash_version` returns a pointer into static read-only
        // memory or null. `read_c_version_string` bounds the strlen scan.
        unsafe { safety::read_c_version_string(ethash_version()) }
    }
}

// ---------------------------------------------------------------------------
// KawPow  (RVN / CLORE)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-kawpow")]
pub mod kawpow {
    //! # Safety / threading model
    //!
    //! - **Not thread-safe.** Per-epoch progpow cache is global; serialise
    //!   calls or pin to a single executor.
    //! - All `header` inputs are fixed at exactly 32 bytes; the safe wrappers
    //!   enforce that via the `&[u8; 32]` parameter type.
    //! - The `*_version()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Compute (mix, hash) for a 32-byte progpow header.
        ///
        /// # Safety
        /// - `header` must be valid for 32 readable bytes.
        /// - `mix_out` and `hash_out` must each be valid for 32 writable
        ///   bytes and may not alias each other or `header`.
        /// - Caller serialises against the global progpow cache.
        pub fn kawpow_hash(
            header: *const u8,
            nonce: u64,
            height: u32,
            epoch: u32,
            mix_out: *mut u8,
            hash_out: *mut u8,
        );

        /// Verify a progpow share.
        ///
        /// # Safety
        /// Same rules as [`kawpow_hash`]. `expected_mix` may be null to skip
        /// the mix-hash equality check; otherwise it must point to 32 bytes.
        /// `target` must point to 32 bytes. Returns `1`/`0`; other values are
        /// ABI breaks.
        pub fn kawpow_verify(
            header: *const u8,
            nonce: u64,
            height: u32,
            epoch: u32,
            expected_mix: *const u8, // may be null
            target: *const u8,
        ) -> i32;

        /// Pure derivation, thread-safe, no init needed.
        pub fn kawpow_get_epoch(height: u32) -> u32;

        /// CPU microbenchmark; same threading rules as [`kawpow_hash`].
        pub fn kawpow_benchmark_cpu(iterations: i32) -> f64;

        /// `'static` read-only version literal; must not be freed.
        pub fn kawpow_version() -> *const std::ffi::c_char;
    }

    /// Returns (mix_hash, final_hash) tuple, each 32 bytes.
    pub fn hash(header: &[u8; 32], nonce: u64, height: u32) -> ([u8; 32], [u8; 32]) {
        let mut mix = [0u8; 32];
        let mut out = [0u8; 32];
        // SAFETY: `kawpow_get_epoch` is pure / thread-safe; `kawpow_hash`
        // requires 32-byte header (enforced by `&[u8; 32]`) and two
        // non-aliasing 32-byte output buffers (fresh stack allocations here).
        let epoch = unsafe { kawpow_get_epoch(height) };
        unsafe {
            kawpow_hash(
                header.as_ptr(),
                nonce,
                height,
                epoch,
                mix.as_mut_ptr(),
                out.as_mut_ptr(),
            );
        }
        (mix, out)
    }

    /// Verify with difficulty target; pass `None` for `expected_mix` to skip mix check.
    pub fn verify(
        header: &[u8; 32],
        nonce: u64,
        height: u32,
        expected_mix: Option<&[u8; 32]>,
        target: &[u8; 32],
    ) -> bool {
        let epoch = unsafe { kawpow_get_epoch(height) };
        let mix_ptr = expected_mix.map_or(std::ptr::null(), |m| m.as_ptr());
        // SAFETY: `header`/`target` are guaranteed 32 bytes; `mix_ptr` is
        // either null (per ABI) or 32 valid bytes from a `&[u8; 32]`.
        unsafe {
            kawpow_verify(
                header.as_ptr(),
                nonce,
                height,
                epoch,
                mix_ptr,
                target.as_ptr(),
            ) == 1
        }
    }

    /// Strict variant of [`verify`] that flags non-`{0,1}` C return codes
    /// as [`FfiError::UnexpectedReturnCode`].
    pub fn try_verify(
        header: &[u8; 32],
        nonce: u64,
        height: u32,
        expected_mix: Option<&[u8; 32]>,
        target: &[u8; 32],
    ) -> Result<bool, FfiError> {
        let epoch = unsafe { kawpow_get_epoch(height) };
        let mix_ptr = expected_mix.map_or(std::ptr::null(), |m| m.as_ptr());
        // SAFETY: see `verify`.
        let code = unsafe {
            kawpow_verify(
                header.as_ptr(),
                nonce,
                height,
                epoch,
                mix_ptr,
                target.as_ptr(),
            )
        };
        safety::parse_c_bool("kawpow_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        // SAFETY: respects module-level threading contract.
        unsafe { kawpow_benchmark_cpu(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `kawpow_version` returns a `'static` literal or null.
        unsafe { safety::read_c_version_string(kawpow_version()) }
    }
}

// ---------------------------------------------------------------------------
// Autolykos v2  (ERG)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-autolykos")]
pub mod autolykos {
    //! # Safety / threading model
    //!
    //! - **Re-entrant / thread-safe.** No global mutable state; multiple
    //!   threads may call [`hash`] / [`verify_u64`] concurrently with
    //!   independent inputs.
    //! - This module does not expose a `*_version()` C symbol.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Compute Autolykos v2 hash.
        ///
        /// # Safety
        /// - `header` must be valid for `header_len` readable bytes.
        /// - `output` must be valid for 32 writable bytes and may not alias
        ///   `header`.
        ///
        /// # Returns
        /// The first 8 bytes of `output` interpreted as a little-endian
        /// `u64`, for caller convenience.
        pub fn autolykos_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            output: *mut u8,
        ) -> u64;

        /// Verify a share against a 64-bit difficulty target.
        ///
        /// # Safety
        /// Same pointer rules as [`autolykos_hash`]. Returns `1`/`0`; other
        /// values are ABI breaks.
        pub fn autolykos_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            target: u64,
        ) -> i32;

        /// CPU microbenchmark; thread-safe.
        pub fn autolykos_benchmark_cpu(iterations: i32) -> f64;
    }

    /// Compute Autolykos v2 hash.  Returns 32-byte output hash.
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: `header` slice is valid for its length; `out` is a fresh
        // 32-byte stack array, non-aliasing.
        unsafe {
            autolykos_hash(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                out.as_mut_ptr(),
            );
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    pub fn try_hash(header: &[u8], nonce: u64, height: u32) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        Ok(hash(header, nonce, height))
    }

    /// Returns `true` if the hash value (first 8 bytes as LE u64) is below `target`.
    pub fn verify_u64(header: &[u8], nonce: u64, height: u32, target: u64) -> bool {
        // SAFETY: see `hash`.
        unsafe { autolykos_verify(header.as_ptr(), header.len(), nonce, height, target) == 1 }
    }

    /// Strict variant of [`verify_u64`] surfacing unexpected C return codes.
    pub fn try_verify_u64(
        header: &[u8],
        nonce: u64,
        height: u32,
        target: u64,
    ) -> Result<bool, FfiError> {
        safety::validate_input_len(header)?;
        // SAFETY: see `hash`.
        let code =
            unsafe { autolykos_verify(header.as_ptr(), header.len(), nonce, height, target) };
        safety::parse_c_bool("autolykos_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        // SAFETY: thread-safe per module docs.
        unsafe { autolykos_benchmark_cpu(iterations) }
    }
}

// ---------------------------------------------------------------------------
// kHeavyHash  (KAS)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-kheavyhash")]
pub mod kheavyhash {
    //! # Safety / threading model
    //!
    //! - **Re-entrant / thread-safe.** No global mutable state.
    //! - The `*_version()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Plain Keccak-derived hash of `input` into 32 bytes.
        ///
        /// # Safety
        /// - `input` must be valid for `len` readable bytes.
        /// - `output` must be valid for 32 writable bytes, non-aliasing.
        pub fn kheavyhash_hash(input: *const u8, len: usize, output: *mut u8);

        /// Mining variant: compute the full kHeavyHash for a Kaspa block
        /// candidate.  `pre_pow_hash` is the 32-byte pre-pow hash, `timestamp`
        /// is the block timestamp (Unix seconds), and `nonce` is the 64-bit
        /// nonce.  Output is 32 bytes.
        ///
        /// # Safety
        /// Same as [`kheavyhash_hash`] for `pre_pow_hash` / `output`.
        pub fn kheavyhash_mine(
            pre_pow_hash: *const u8,
            pre_pow_hash_len: usize,
            timestamp: u64,
            nonce: u64,
            output: *mut u8,
        );

        /// Verify mining hash against 32-byte target.
        ///
        /// # Safety
        /// `pre_pow_hash` and `target` must be valid for their respective
        /// lengths.  Returns `1`/`0`; other values are ABI breaks.
        pub fn kheavyhash_verify(
            pre_pow_hash: *const u8,
            pre_pow_hash_len: usize,
            timestamp: u64,
            nonce: u64,
            target: *const u8,
        ) -> i32;

        /// CPU microbenchmark; thread-safe.
        pub fn kheavyhash_benchmark(iterations: i32) -> f64;

        /// `'static` read-only version literal; must not be freed.
        pub fn kheavyhash_version() -> *const std::ffi::c_char;
    }

    pub fn hash(input: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: slice + fresh 32-byte stack output, non-aliasing.
        unsafe {
            kheavyhash_hash(input.as_ptr(), input.len(), out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    pub fn try_hash(input: &[u8]) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(input)?;
        Ok(hash(input))
    }

    /// Mining variant: compute the full kHeavyHash for a Kaspa block candidate.
    ///
    /// `pre_pow_hash` is the 32-byte pre-pow hash, `timestamp` is the block
    /// timestamp (Unix seconds), and `nonce` is the 64-bit nonce.
    pub fn mine(pre_pow_hash: &[u8], timestamp: u64, nonce: u64) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: same as `hash`.
        unsafe {
            kheavyhash_mine(
                pre_pow_hash.as_ptr(),
                pre_pow_hash.len(),
                timestamp,
                nonce,
                out.as_mut_ptr(),
            );
        }
        out
    }

    /// Fallible variant of [`mine`] that rejects empty / oversized inputs.
    pub fn try_mine(
        pre_pow_hash: &[u8],
        timestamp: u64,
        nonce: u64,
    ) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(pre_pow_hash)?;
        Ok(mine(pre_pow_hash, timestamp, nonce))
    }

    pub fn verify(
        pre_pow_hash: &[u8],
        timestamp: u64,
        nonce: u64,
        target: &[u8; 32],
    ) -> bool {
        // SAFETY: `pre_pow_hash`/`target` valid; thread-safe.
        unsafe {
            kheavyhash_verify(
                pre_pow_hash.as_ptr(),
                pre_pow_hash.len(),
                timestamp,
                nonce,
                target.as_ptr(),
            ) == 1
        }
    }

    /// Strict variant of [`verify`] surfacing unexpected C return codes.
    pub fn try_verify(
        pre_pow_hash: &[u8],
        timestamp: u64,
        nonce: u64,
        target: &[u8; 32],
    ) -> Result<bool, FfiError> {
        safety::validate_input_len(pre_pow_hash)?;
        // SAFETY: see `verify`.
        let code = unsafe {
            kheavyhash_verify(
                pre_pow_hash.as_ptr(),
                pre_pow_hash.len(),
                timestamp,
                nonce,
                target.as_ptr(),
            )
        };
        safety::parse_c_bool("kheavyhash_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        // SAFETY: thread-safe.
        unsafe { kheavyhash_benchmark(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `'static` literal or null.
        unsafe { safety::read_c_version_string(kheavyhash_version()) }
    }
}

// ---------------------------------------------------------------------------
// Blake3-algo  (ALPH, DCR)
// Feature name is blake3-algo to avoid collision with the blake3 pure-Rust
// crate used elsewhere in the workspace.
// ---------------------------------------------------------------------------

#[cfg(feature = "native-blake3-algo")]
pub mod blake3_algo {
    //! # Safety / threading model
    //!
    //! - **Re-entrant / thread-safe.** Pure stateless hash core.
    //! - The `*_version()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Compute Blake3 of `input` into 32 bytes.
        ///
        /// # Safety
        /// - `input` must be valid for `input_len` readable bytes.
        /// - `output` must be valid for 32 writable bytes, non-aliasing.
        pub fn blake3_hash(input: *const u8, input_len: usize, output: *mut u8);

        /// Mining variant: hash `(header || nonce_le)` into 32 bytes.
        ///
        /// # Safety
        /// Same as [`blake3_hash`].
        pub fn blake3_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);

        /// Verify mining hash against 32-byte target.
        ///
        /// # Safety
        /// `header` and `target` must be valid for their respective lengths.
        /// Returns `1`/`0`; other values are ABI breaks.
        pub fn blake3_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            target: *const u8,
        ) -> i32;

        /// CPU microbenchmark; thread-safe.
        pub fn blake3_benchmark(iterations: i32) -> f64;

        /// `'static` read-only version literal; must not be freed.
        pub fn blake3_version() -> *const std::ffi::c_char;
    }

    pub fn hash(input: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: slice + fresh 32-byte stack output, non-aliasing.
        unsafe {
            blake3_hash(input.as_ptr(), input.len(), out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    pub fn try_hash(input: &[u8]) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(input)?;
        Ok(hash(input))
    }

    pub fn mine(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: same as `hash`.
        unsafe {
            blake3_mine(header.as_ptr(), header.len(), nonce, out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`mine`] that rejects empty / oversized inputs.
    pub fn try_mine(header: &[u8], nonce: u64) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        Ok(mine(header, nonce))
    }

    pub fn verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> bool {
        // SAFETY: thread-safe; pointers valid.
        unsafe { blake3_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) == 1 }
    }

    /// Strict variant of [`verify`] surfacing unexpected C return codes.
    pub fn try_verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> Result<bool, FfiError> {
        safety::validate_input_len(header)?;
        // SAFETY: see `verify`.
        let code = unsafe { blake3_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) };
        safety::parse_c_bool("blake3_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        // SAFETY: thread-safe.
        unsafe { blake3_benchmark(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `'static` literal or null.
        unsafe { safety::read_c_version_string(blake3_version()) }
    }
}

// ---------------------------------------------------------------------------
// Cosmic Harmony v3  (ZION)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-cosmic-harmony")]
pub mod cosmic_harmony {
    //! # Safety / threading model
    //!
    //! - **Re-entrant / thread-safe.** The C-side CHv3 pipeline operates on
    //!   per-call stack scratch (Keccak-256 → SHA3-512 → Golden Matrix →
    //!   Cosmic Fusion); no shared mutable state.
    //! - C entry points return `0` on success; non-zero values are ABI
    //!   breaks and surface as [`FfiError::UnexpectedReturnCode`] from
    //!   the `try_*` variants.
    //! - The `*_get_info()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Full CHv3 pipeline through nonce.
        ///
        /// # Safety
        /// - `header` must be valid for `header_len` readable bytes.
        /// - `output` must be valid for 32 writable bytes, non-aliasing.
        ///
        /// # Returns
        /// `0` on success; non-zero indicates an internal error.
        pub fn cosmic_harmony_v3_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            output: *mut u8,
        ) -> i32;

        /// CHv3 over raw bytes (no implicit nonce concatenation).
        ///
        /// # Safety
        /// Same pointer rules as [`cosmic_harmony_v3_hash`].
        pub fn cosmic_harmony_v3_hash_raw(
            input: *const u8,
            input_len: usize,
            output: *mut u8,
        ) -> i32;

        /// Run for `duration_seconds` wallclock seconds and return H/s.
        pub fn cosmic_harmony_v3_benchmark(duration_seconds: i32) -> f64;

        /// `'static` read-only info literal; must not be freed.
        pub fn cosmic_harmony_v3_get_info() -> *const std::ffi::c_char;
    }

    /// Hash block header with nonce appended; returns 32-byte CHv3 output.
    pub fn mine(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: slice + fresh 32-byte stack output, non-aliasing.
        // We deliberately ignore the return code in this back-compat
        // wrapper; `try_mine` surfaces it.
        unsafe {
            cosmic_harmony_v3_hash(header.as_ptr(), header.len(), nonce, out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`mine`] that rejects empty / oversized inputs and
    /// surfaces a non-zero C return code as
    /// [`FfiError::UnexpectedReturnCode`].
    pub fn try_mine(header: &[u8], nonce: u64) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        let mut out = [0u8; 32];
        // SAFETY: see `mine`.
        let code = unsafe {
            cosmic_harmony_v3_hash(header.as_ptr(), header.len(), nonce, out.as_mut_ptr())
        };
        if code != 0 {
            return Err(FfiError::UnexpectedReturnCode {
                c_function: "cosmic_harmony_v3_hash",
                code,
            });
        }
        Ok(out)
    }

    /// Hash raw bytes (no nonce appended internally); returns 32-byte CHv3 output.
    pub fn hash_raw(input: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        // SAFETY: slice + fresh 32-byte stack output, non-aliasing.
        unsafe {
            cosmic_harmony_v3_hash_raw(input.as_ptr(), input.len(), out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`hash_raw`] that rejects empty / oversized inputs
    /// and surfaces non-zero C return codes.
    pub fn try_hash_raw(input: &[u8]) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(input)?;
        let mut out = [0u8; 32];
        // SAFETY: see `hash_raw`.
        let code =
            unsafe { cosmic_harmony_v3_hash_raw(input.as_ptr(), input.len(), out.as_mut_ptr()) };
        if code != 0 {
            return Err(FfiError::UnexpectedReturnCode {
                c_function: "cosmic_harmony_v3_hash_raw",
                code,
            });
        }
        Ok(out)
    }

    /// Run benchmark for `duration_secs` seconds; returns hashes/second.
    pub fn benchmark(duration_secs: i32) -> f64 {
        // SAFETY: thread-safe.
        unsafe { cosmic_harmony_v3_benchmark(duration_secs) }
    }

    /// Return the C library's `cosmic_harmony_v3_get_info()` string, or an
    /// [`FfiError`] if the pointer is null / unterminated.
    pub fn info() -> Result<String, FfiError> {
        // SAFETY: `'static` literal or null.
        unsafe { safety::read_c_version_string(cosmic_harmony_v3_get_info()) }
    }
}

// ---------------------------------------------------------------------------
// VerusHash v2.2  (VRSC)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-verushash")]
pub mod verushash {
    //! # Safety / threading model
    //!
    //! - **Thread-safe after init.** [`init`] is wrapped in a `std::sync::Once`
    //!   and invoked transparently by every safe wrapper before the first
    //!   `hash`/`verify`/`benchmark` call. After init returns, the C-side
    //!   precomputed tables are read-only and the per-call hash routines are
    //!   re-entrant.
    //! - The `*_version()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Build the precomputed tables for VerusHash v2.2.
        ///
        /// # Safety
        /// May allocate; thread-safety is provided externally via the
        /// `Once` in this module.
        pub fn verushash_init();

        /// Compute VerusHash v2.2 of `(header, nonce)` into 32 bytes.
        ///
        /// # Safety
        /// - `header` must be valid for `header_len` readable bytes.
        /// - `output` must be valid for 32 writable bytes, non-aliasing.
        /// - `verushash_init` must have completed at least once.
        pub fn verushash_hash(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);

        /// Verify VerusHash v2.2 against 32-byte target.
        ///
        /// # Safety
        /// Same as [`verushash_hash`]. Returns `1`/`0`; other values are ABI
        /// breaks.
        pub fn verushash_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            target: *const u8,
        ) -> i32;

        /// CPU microbenchmark; thread-safe after init.
        pub fn verushash_benchmark(iterations: i32) -> f64;

        /// `'static` read-only version literal; must not be freed.
        pub fn verushash_version() -> *const std::ffi::c_char;
    }

    use std::sync::Once;
    static INIT: Once = Once::new();

    pub fn init() {
        INIT.call_once(|| {
            // SAFETY: `Once` ensures this runs at most once across all
            // threads, so the C-side init is single-threaded as it requires.
            unsafe {
                verushash_init();
            }
        });
    }

    pub fn hash(header: &[u8], nonce: u64) -> [u8; 32] {
        init();
        let mut out = [0u8; 32];
        // SAFETY: init has completed; slice + fresh 32-byte stack output.
        unsafe {
            verushash_hash(header.as_ptr(), header.len(), nonce, out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    pub fn try_hash(header: &[u8], nonce: u64) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        Ok(hash(header, nonce))
    }

    pub fn verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> bool {
        init();
        // SAFETY: init has completed; pointers valid.
        unsafe { verushash_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) == 1 }
    }

    /// Strict variant of [`verify`] surfacing unexpected C return codes.
    pub fn try_verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> Result<bool, FfiError> {
        safety::validate_input_len(header)?;
        init();
        // SAFETY: see `verify`.
        let code =
            unsafe { verushash_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) };
        safety::parse_c_bool("verushash_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        init();
        // SAFETY: thread-safe after init.
        unsafe { verushash_benchmark(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `'static` literal or null.
        unsafe { safety::read_c_version_string(verushash_version()) }
    }
}

// ---------------------------------------------------------------------------
// RandomX  (XMR, ZEPH)
// ---------------------------------------------------------------------------

#[cfg(feature = "native-randomx")]
pub mod randomx {
    //! # Safety / threading model
    //!
    //! - **Thread-safe after init.** [`init`] is wrapped in a `std::sync::Once`
    //!   and invoked transparently by every safe wrapper. After init returns,
    //!   the C-side dataset/cache is treated as read-only and per-call
    //!   hashing is re-entrant.
    //! - The `*_version()` pointer is `'static` read-only memory.
    use super::safety::{self, FfiError};

    unsafe extern "C" {
        /// Build the RandomX dataset / VM caches.
        ///
        /// # Safety
        /// May allocate large memory regions; thread-safety is provided
        /// externally via the `Once` in this module.
        pub fn randomx_zion_init();

        /// Compute RandomX-Zion of `(header, nonce)` into 32 bytes.
        ///
        /// # Safety
        /// - `header` must be valid for `header_len` readable bytes.
        /// - `output` must be valid for 32 writable bytes, non-aliasing.
        /// - `randomx_zion_init` must have completed at least once.
        pub fn randomx_zion_hash(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);

        /// Verify RandomX-Zion against 32-byte target.
        ///
        /// # Safety
        /// Same as [`randomx_zion_hash`]. Returns `1`/`0`; other values are
        /// ABI breaks.
        pub fn randomx_zion_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            target: *const u8,
        ) -> i32;

        /// CPU microbenchmark; thread-safe after init.
        pub fn randomx_zion_benchmark(iterations: i32) -> f64;

        /// `'static` read-only version literal; must not be freed.
        pub fn randomx_zion_version() -> *const std::ffi::c_char;
    }

    use std::sync::Once;
    static INIT: Once = Once::new();

    pub fn init() {
        INIT.call_once(|| {
            // SAFETY: `Once` ensures the C-side init runs at most once.
            unsafe {
                randomx_zion_init();
            }
        });
    }

    pub fn hash(header: &[u8], nonce: u64) -> [u8; 32] {
        init();
        let mut out = [0u8; 32];
        // SAFETY: init has completed; slice + fresh 32-byte stack output.
        unsafe {
            randomx_zion_hash(header.as_ptr(), header.len(), nonce, out.as_mut_ptr());
        }
        out
    }

    /// Fallible variant of [`hash`] that rejects empty / oversized inputs.
    pub fn try_hash(header: &[u8], nonce: u64) -> Result<[u8; 32], FfiError> {
        safety::validate_input_len(header)?;
        Ok(hash(header, nonce))
    }

    pub fn verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> bool {
        init();
        // SAFETY: init has completed; pointers valid.
        unsafe { randomx_zion_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) == 1 }
    }

    /// Strict variant of [`verify`] surfacing unexpected C return codes.
    pub fn try_verify(header: &[u8], nonce: u64, target: &[u8; 32]) -> Result<bool, FfiError> {
        safety::validate_input_len(header)?;
        init();
        // SAFETY: see `verify`.
        let code =
            unsafe { randomx_zion_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) };
        safety::parse_c_bool("randomx_zion_verify", code)
    }

    pub fn benchmark(iterations: i32) -> f64 {
        init();
        // SAFETY: thread-safe after init.
        unsafe { randomx_zion_benchmark(iterations) }
    }

    /// Return the C library's version string, or an [`FfiError`] if the
    /// pointer is null / unterminated.
    pub fn version() -> Result<String, FfiError> {
        // SAFETY: `'static` literal or null.
        unsafe { safety::read_c_version_string(randomx_zion_version()) }
    }
}

// ---------------------------------------------------------------------------
// Algorithm registry  — enumerate which features are compiled in
// ---------------------------------------------------------------------------

/// Returns the list of native algorithm names compiled into this build.
#[allow(unused_mut)] // mutated only when a native-* feature is enabled
pub fn compiled_algorithms() -> Vec<&'static str> {
    let mut v = Vec::new();
    #[cfg(feature = "native-etchash")]
    {
        v.push("etchash");
    }
    #[cfg(feature = "native-kawpow")]
    {
        v.push("kawpow");
    }
    #[cfg(feature = "native-autolykos")]
    {
        v.push("autolykos");
    }
    #[cfg(feature = "native-kheavyhash")]
    {
        v.push("kheavyhash");
    }
    #[cfg(feature = "native-blake3-algo")]
    {
        v.push("blake3");
    }
    #[cfg(feature = "native-cosmic-harmony")]
    {
        v.push("cosmic-harmony");
    }
    #[cfg(feature = "native-verushash")]
    {
        v.push("verushash");
    }
    #[cfg(feature = "native-randomx")]
    {
        v.push("randomx");
    }
    v
}

// ---------------------------------------------------------------------------
// Runtime self-test — validates each compiled algorithm against a canonical
// deterministic check at startup.  Returns a list of (algo_name, passed) pairs.
// ---------------------------------------------------------------------------

/// Result of a single algorithm self-test.
#[derive(Debug, Clone)]
pub struct AlgoTestResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Run deterministic self-tests for every compiled algorithm.
///
/// Each test computes a hash with a fixed input and verifies:
/// 1. The output is non-zero (symbol loaded correctly).
/// 2. A second invocation produces the same output (determinism).
///
/// Call this once at miner startup.  If any result has `passed == false`,
/// the corresponding algorithm should not be used for real mining.
#[allow(unused_mut)] // mutated only when a native-* feature is enabled
pub fn runtime_self_test() -> Vec<AlgoTestResult> {
    let mut results = Vec::new();

    #[cfg(feature = "native-etchash")]
    {
        let name = "etchash";
        let header = [0xA1u8; 32];
        let h1 = etchash::hash(&header, 1, 0);
        let h2 = etchash::hash(&header, 1, 0);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-kawpow")]
    {
        let name = "kawpow";
        let header = [0xA2u8; 32];
        let (_, h1) = kawpow::hash(&header, 1, 0);
        let (_, h2) = kawpow::hash(&header, 1, 0);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-autolykos")]
    {
        let name = "autolykos";
        let header = [0xA3u8; 32];
        let h1 = autolykos::hash(&header, 1, 700_000);
        let h2 = autolykos::hash(&header, 1, 700_000);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-kheavyhash")]
    {
        let name = "kheavyhash";
        let header = [0xA4u8; 32];
        let h1 = kheavyhash::mine(&header, 5_435_345_234, 1);
        let h2 = kheavyhash::mine(&header, 5_435_345_234, 1);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-blake3-algo")]
    {
        let name = "blake3";
        let header = [0xA5u8; 32];
        let h1 = blake3_algo::mine(&header, 1);
        let h2 = blake3_algo::mine(&header, 1);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-cosmic-harmony")]
    {
        let name = "cosmic-harmony";
        let header = [0xA6u8; 80];
        let h1 = cosmic_harmony::mine(&header, 1);
        let h2 = cosmic_harmony::mine(&header, 1);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-verushash")]
    {
        let name = "verushash";
        let header = [0xA7u8; 76];
        let h1 = verushash::hash(&header, 1);
        let h2 = verushash::hash(&header, 1);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    #[cfg(feature = "native-randomx")]
    {
        let name = "randomx";
        let header = [0xA8u8; 76];
        let h1 = randomx::hash(&header, 1);
        let h2 = randomx::hash(&header, 1);
        let ok = h1 != [0u8; 32] && h1 == h2;
        results.push(AlgoTestResult {
            name,
            passed: ok,
            detail: if ok {
                "deterministic, non-zero".into()
            } else {
                "FAILED: zero or non-deterministic".into()
            },
        });
    }

    results
}

/// Returns `true` if all compiled algorithms pass their self-test.
pub fn all_algorithms_healthy() -> bool {
    runtime_self_test().iter().all(|r| r.passed)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_algorithms_baseline() {
        // Always passes — just documents which algos are in this build.
        let algos = compiled_algorithms();
        println!("zion-native-ffi compiled algorithms: {:?}", algos);
    }

    #[test]
    fn runtime_self_test_all_pass() {
        let results = runtime_self_test();
        println!("runtime_self_test: {} algos tested", results.len());
        for r in &results {
            println!(
                "  {} — {} ({})",
                r.name,
                if r.passed { "OK" } else { "FAIL" },
                r.detail
            );
            assert!(
                r.passed,
                "algorithm {} failed self-test: {}",
                r.name, r.detail
            );
        }
    }

    #[test]
    fn all_algorithms_healthy_passes() {
        assert!(all_algorithms_healthy() || compiled_algorithms().is_empty());
    }

    #[test]
    fn self_test_count_matches_compiled() {
        let compiled = compiled_algorithms().len();
        let tested = runtime_self_test().len();
        assert_eq!(
            compiled, tested,
            "every compiled algo must have a self-test"
        );
    }

    #[cfg(feature = "native-etchash")]
    #[test]
    fn etchash_smoke() {
        etchash::init();
        let header = [0x01u8; 32];
        let hash = etchash::hash(&header, 12345, 0);
        assert_ne!(hash, [0u8; 32], "etchash must produce non-zero output");
        println!("etchash smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-kawpow")]
    #[test]
    fn kawpow_smoke() {
        let header = [0x02u8; 32];
        let (_mix, hash) = kawpow::hash(&header, 99999, 1_000_000);
        assert_ne!(hash, [0u8; 32], "kawpow must produce non-zero output");
        println!("kawpow smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-autolykos")]
    #[test]
    fn autolykos_smoke() {
        let header = [0x03u8; 32];
        let hash = autolykos::hash(&header, 42, 700_000);
        assert_ne!(hash, [0u8; 32], "autolykos must produce non-zero output");
        println!("autolykos smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-kheavyhash")]
    #[test]
    fn kheavyhash_smoke() {
        let header = [0x04u8; 32];
        let hash = kheavyhash::mine(&header, 5_435_345_234, 1234);
        assert_ne!(hash, [0u8; 32], "kheavyhash must produce non-zero output");
        println!("kheavyhash smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-blake3-algo")]
    #[test]
    fn blake3_algo_smoke() {
        let header = [0x05u8; 32];
        let hash = blake3_algo::mine(&header, 5678);
        assert_ne!(hash, [0u8; 32], "blake3-algo must produce non-zero output");
        println!("blake3 smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-cosmic-harmony")]
    #[test]
    fn cosmic_harmony_smoke() {
        let header = [0x06u8; 80];
        let hash = cosmic_harmony::mine(&header, 7890);
        assert_ne!(
            hash, [0u8; 32],
            "cosmic-harmony must produce non-zero output"
        );
        let h2 = cosmic_harmony::mine(&header, 7890);
        assert_eq!(hash, h2, "cosmic-harmony must be deterministic");
        println!("cosmic-harmony smoke: {:02x?}", &hash[..8]);
    }

    #[cfg(feature = "native-verushash")]
    #[test]
    fn verushash_smoke() {
        let header = [0x07u8; 76];
        let h1 = verushash::hash(&header, 0);
        let h2 = verushash::hash(&header, 0);
        assert_eq!(h1, h2, "verushash must be deterministic");
        assert_ne!(h1, [0u8; 32], "verushash must produce non-zero output");
        println!("verushash smoke: {:02x?}", &h1[..8]);
    }

    #[cfg(feature = "native-randomx")]
    #[test]
    fn randomx_smoke() {
        let header = [0x08u8; 76];
        let h1 = randomx::hash(&header, 0);
        let h2 = randomx::hash(&header, 0);
        assert_eq!(h1, h2, "randomx must be deterministic");
        assert_ne!(h1, [0u8; 32], "randomx must produce non-zero output");
        println!("randomx smoke: {:02x?}", &h1[..8]);
    }

    // ----------------------------------------------------------------------
    // FFI safety-contract regression tests.
    // ----------------------------------------------------------------------
    //
    // These exercise the shared `safety` helpers (and per-module `try_*`
    // wrappers when a feature is compiled in) so the fail-closed length
    // validation, version-string scan cap, and 0/1 bool parser are pinned
    // against future regressions.

    mod safety_contract {
        use super::super::safety::{self, FfiError, MAX_C_STRING_SCAN_BYTES, MAX_INPUT_LEN_BYTES};

        #[test]
        fn validate_input_len_rejects_empty() {
            let err = safety::validate_input_len(&[])
                .expect_err("empty slice must be rejected by validate_input_len");
            assert_eq!(err, FfiError::EmptyInput);
            assert!(err.to_string().contains("ffi_input_empty"));
        }

        #[test]
        fn validate_input_len_rejects_oversized() {
            // Use a virtual length: we cannot allocate >1 MiB on the test stack,
            // but we don't need to — the helper only inspects len/is_empty.
            let payload = vec![0u8; MAX_INPUT_LEN_BYTES + 1];
            let err = safety::validate_input_len(&payload).expect_err("oversized must reject");
            match err {
                FfiError::InputTooLarge { len, max } => {
                    assert_eq!(len, MAX_INPUT_LEN_BYTES + 1);
                    assert_eq!(max, MAX_INPUT_LEN_BYTES);
                }
                other => panic!("expected InputTooLarge, got {other:?}"),
            }
        }

        #[test]
        fn validate_input_len_accepts_typical_header() {
            // 80-byte block header — well under cap.
            let payload = [0xABu8; 80];
            assert!(safety::validate_input_len(&payload).is_ok());
        }

        #[test]
        fn read_c_version_string_rejects_null() {
            // SAFETY: explicitly testing the null-pointer guard.
            let err = unsafe { safety::read_c_version_string(std::ptr::null()) }
                .expect_err("null pointer must be rejected");
            assert_eq!(err, FfiError::NullVersionString);
        }

        #[test]
        fn read_c_version_string_decodes_valid_static_literal() {
            let s = c"zion-native-ffi/test-1.2.3";
            // SAFETY: `c"..."` produces a null-terminated `&CStr`; its
            // pointer is valid for the lifetime of the program.
            let got = unsafe { safety::read_c_version_string(s.as_ptr()) }
                .expect("valid static literal must decode");
            assert_eq!(got, "zion-native-ffi/test-1.2.3");
        }

        #[test]
        fn read_c_version_string_caps_unterminated_scan() {
            // Build an unterminated buffer of exactly MAX_C_STRING_SCAN_BYTES
            // non-zero bytes. The helper must report `UnterminatedVersionString`
            // before falling off the end (i.e. before reading byte
            // MAX + 1, which would be undefined-behaviour territory).
            let buf = vec![0xCDu8; MAX_C_STRING_SCAN_BYTES];
            // SAFETY: pointer is non-null and points to MAX bytes of valid
            // memory we own. The helper's strnlen-equivalent stops at the
            // cap; we never read past `buf`.
            let err = unsafe { safety::read_c_version_string(buf.as_ptr() as *const _) }
                .expect_err("unterminated buffer must be rejected at the cap");
            match err {
                FfiError::UnterminatedVersionString { scanned } => {
                    assert_eq!(scanned, MAX_C_STRING_SCAN_BYTES);
                }
                other => panic!("expected UnterminatedVersionString, got {other:?}"),
            }
        }

        #[test]
        fn parse_c_bool_accepts_canonical_values() {
            assert_eq!(safety::parse_c_bool("test_fn", 0), Ok(false));
            assert_eq!(safety::parse_c_bool("test_fn", 1), Ok(true));
        }

        #[test]
        fn parse_c_bool_flags_abi_break() {
            let err = safety::parse_c_bool("test_fn", -1)
                .expect_err("non-{0,1} return must surface as FfiError");
            match err {
                FfiError::UnexpectedReturnCode { c_function, code } => {
                    assert_eq!(c_function, "test_fn");
                    assert_eq!(code, -1);
                }
                other => panic!("expected UnexpectedReturnCode, got {other:?}"),
            }
            let err2 = safety::parse_c_bool("test_fn", 42).unwrap_err();
            assert!(matches!(
                err2,
                FfiError::UnexpectedReturnCode { code: 42, .. }
            ));
        }

        #[test]
        fn ffi_error_display_carries_failure_class_for_ops_triage() {
            let cases: &[(FfiError, &str)] = &[
                (FfiError::EmptyInput, "ffi_input_empty"),
                (
                    FfiError::InputTooLarge {
                        len: 5,
                        max: MAX_INPUT_LEN_BYTES,
                    },
                    "ffi_input_too_large",
                ),
                (FfiError::NullVersionString, "ffi_version_null_pointer"),
                (
                    FfiError::UnterminatedVersionString { scanned: 4096 },
                    "ffi_version_unterminated",
                ),
                (
                    FfiError::UnexpectedReturnCode {
                        c_function: "x",
                        code: -2,
                    },
                    "ffi_unexpected_return_code",
                ),
            ];
            for (err, needle) in cases {
                let s = err.to_string();
                assert!(
                    s.contains(needle),
                    "Display impl for {err:?} must contain {needle:?}; got {s:?}"
                );
            }
        }
    }

    // try_* wrappers per algorithm: empty-input rejection.
    // We cannot easily provoke `UnexpectedReturnCode` from real C code, but
    // we can pin that empty-slice inputs never reach C — the failure mode
    // operators rely on for fail-closed behaviour.

    #[cfg(feature = "native-etchash")]
    #[test]
    fn etchash_try_hash_rejects_empty_input() {
        let err = etchash::try_hash(&[], 0, 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-autolykos")]
    #[test]
    fn autolykos_try_hash_rejects_empty_input() {
        let err = autolykos::try_hash(&[], 0, 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-kheavyhash")]
    #[test]
    fn kheavyhash_try_hash_rejects_empty_input() {
        let err = kheavyhash::try_hash(&[]).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
        let err = kheavyhash::try_mine(&[], 0, 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-blake3-algo")]
    #[test]
    fn blake3_try_hash_rejects_empty_input() {
        let err = blake3_algo::try_hash(&[]).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
        let err = blake3_algo::try_mine(&[], 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-cosmic-harmony")]
    #[test]
    fn cosmic_harmony_try_mine_rejects_empty_input() {
        let err = cosmic_harmony::try_mine(&[], 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
        let err = cosmic_harmony::try_hash_raw(&[]).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-verushash")]
    #[test]
    fn verushash_try_hash_rejects_empty_input() {
        let err = verushash::try_hash(&[], 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }

    #[cfg(feature = "native-randomx")]
    #[test]
    fn randomx_try_hash_rejects_empty_input() {
        let err = randomx::try_hash(&[], 0).expect_err("empty input must reject");
        assert!(matches!(err, super::safety::FfiError::EmptyInput));
    }
}
