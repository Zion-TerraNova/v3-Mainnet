// build.rs — compile native C algorithm libraries based on feature flags.
//
// Each algorithm is an independent cc::Build unit so they can be selectively
// included/excluded at cargo feature resolution time.  Missing a feature never
// prevents baseline miner compilation.
//
// NOTE: #[cfg(feature = "...")] does NOT work in build.rs.
//       Feature presence is checked via CARGO_FEATURE_<NAME> env vars.

use std::env;
use std::path::PathBuf;

fn feat(name: &str) -> bool {
    let key = format!("CARGO_FEATURE_{}", name.to_uppercase().replace('-', "_"));
    env::var(&key).is_ok()
}

/// On Windows MSVC, cc-rs may not find the Windows SDK / VC include paths when
/// invoked from a plain terminal (not a VS Developer Command Prompt).
/// Detect and add them explicitly so C standard headers are resolved.
fn add_msvc_includes(b: &mut cc::Build) {
    // 1. VCToolsInstallDir env var (set by vcvarsall.bat / developer prompt)
    if let Ok(v) = env::var("VCToolsInstallDir") {
        let inc = PathBuf::from(&v).join("include");
        if inc.exists() {
            b.include(&inc);
        }
    }

    // 2. Walk known VS installation roots (VS 2022 + VS 2026)
    let roots: &[&str] = &[
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Tools\\MSVC",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Professional\\VC\\Tools\\MSVC",
        "C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Tools\\MSVC",
        "D:\\VS2026\\VC\\Tools\\MSVC",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Tools\\MSVC",
    ];
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            if let Some(latest) = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .max_by_key(|e| e.file_name())
            {
                let inc = latest.path().join("include");
                if inc.exists() {
                    b.include(&inc);
                    break;
                }
            }
        }
    }

    // 3. Windows SDK ucrt/um/shared headers
    let sdk_roots: &[&str] = &[
        "C:\\Program Files (x86)\\Windows Kits\\10\\Include",
        "C:\\Program Files\\Windows Kits\\10\\Include",
    ];
    for sdk_root in sdk_roots {
        if let Ok(entries) = std::fs::read_dir(sdk_root) {
            if let Some(latest) = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .max_by_key(|e| e.file_name())
            {
                for sub in &["ucrt", "um", "shared"] {
                    let p = latest.path().join(sub);
                    if p.exists() {
                        b.include(&p);
                    }
                }
                break;
            }
        }
    }

    // 4. Force-include the POSIX compat shim (provides clock_gettime etc.)
    b.include("csrc/compat");
    b.flag_if_supported("/FIzion_time_compat.h");
}

/// Apply flags shared across all plain-C algorithm builds.
fn base_build(src: &str, lib: &str, target_os: &str, is_msvc: bool) {
    let mut b = cc::Build::new();
    b.file(src)
        .opt_level(3)
        .warnings(false)
        .cargo_warnings(false);

    if !is_msvc {
        b.flag_if_supported("-fPIC");
        b.flag_if_supported("-funroll-loops");
        b.flag_if_supported("-fomit-frame-pointer");
        if target_os == "linux" {
            b.define("_POSIX_C_SOURCE", "200112L");
        }
    } else {
        b.flag_if_supported("/std:c11");
        add_msvc_includes(&mut b);
    }
    b.compile(lib);
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let is_msvc = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc";

    // -----------------------------------------------------------------------
    // Etchash / Ethash  (ETC, ETCPoW)
    // -----------------------------------------------------------------------
    if feat("native-etchash") {
        let mut b = cc::Build::new();
        b.file("csrc/etchash/etchash_native.c")
            .opt_level(3)
            .warnings(false)
            .cargo_warnings(false);
        if !is_msvc {
            b.flag_if_supported("-fPIC");
            if target_os == "linux" {
                b.define("_POSIX_C_SOURCE", "200112L");
            }
        } else {
            b.flag_if_supported("/std:c11");
            add_msvc_includes(&mut b);
        }
        b.compile("etchash_zion");
        if target_os == "linux" {
            println!("cargo:rustc-link-lib=m");
        }
    }

    // -----------------------------------------------------------------------
    // KawPow  (RVN, CLORE)
    // -----------------------------------------------------------------------
    if feat("native-kawpow") {
        base_build(
            "csrc/kawpow/kawpow_native.c",
            "kawpow_zion",
            &target_os,
            is_msvc,
        );
    }

    // -----------------------------------------------------------------------
    // Autolykos v2  (ERG)
    // -----------------------------------------------------------------------
    if feat("native-autolykos") {
        base_build(
            "csrc/autolykos/autolykos_native.c",
            "autolykos_zion",
            &target_os,
            is_msvc,
        );
    }

    // -----------------------------------------------------------------------
    // kHeavyHash  (KAS)
    // -----------------------------------------------------------------------
    if feat("native-kheavyhash") {
        base_build(
            "csrc/kheavyhash/kheavyhash_native.c",
            "kheavyhash_zion",
            &target_os,
            is_msvc,
        );
    }

    // -----------------------------------------------------------------------
    // Blake3  (ALPH, DCR)  — named blake3-algo to avoid clash with the
    //                         pure-Rust blake3 crate in the workspace
    // -----------------------------------------------------------------------
    if feat("native-blake3-algo") {
        base_build(
            "csrc/blake3/blake3_native.c",
            "blake3_algo_zion",
            &target_os,
            is_msvc,
        );
    }

    // -----------------------------------------------------------------------
    // Cosmic Harmony v3  (ZION)
    // -----------------------------------------------------------------------
    if feat("native-cosmic-harmony") {
        let mut b = cc::Build::new();
        b.file("csrc/cosmic_harmony/cosmic_harmony_v3_native.c")
            .opt_level(3)
            .warnings(false)
            .cargo_warnings(false);
        if !is_msvc {
            b.flag_if_supported("-fPIC");
            b.flag_if_supported("-funroll-loops");
            if target_arch == "x86_64" {
                b.flag_if_supported("-mavx2");
            }
            if target_os == "linux" {
                b.define("_POSIX_C_SOURCE", "200112L");
            }
        } else {
            b.flag_if_supported("/std:c11");
            b.flag_if_supported("/arch:AVX2");
            add_msvc_includes(&mut b);
        }
        b.compile("cosmic_harmony_zion");
    }

    // -----------------------------------------------------------------------
    // VerusHash v2.2  (VRSC)
    //   Production: Haraka + CLHash pipeline from VerusCoin upstream.
    //   Sources in csrc/verushash/real/ (downloaded from VerusCoin GitHub).
    //   Falls back to portable stub if real sources are not present.
    // -----------------------------------------------------------------------
    if feat("native-verushash") {
        let real_dir = "csrc/verushash/real";
        let has_real = std::path::Path::new(real_dir).join("verus_hash.cpp").exists();

        if has_real {
            // --- Production build: compile real VerusHash C++ sources ---
            // 1. Compile pure-C sources (haraka)
            let mut c_build = cc::Build::new();
            c_build
                .file("csrc/verushash/real/haraka.c")
                .file("csrc/verushash/real/haraka_portable.c")
                .include("csrc/verushash/real")
                .opt_level(3)
                .warnings(false)
                .cargo_warnings(false)
                .flag_if_supported("-funroll-loops")
                .flag_if_supported("-fomit-frame-pointer")
                .flag_if_supported("-fPIC");
            if !is_msvc {
                if target_arch == "x86_64" {
                    c_build
                        .flag_if_supported("-mpclmul")
                        .flag_if_supported("-msse4")
                        .flag_if_supported("-msse4.1")
                        .flag_if_supported("-msse4.2")
                        .flag_if_supported("-mssse3")
                        .flag_if_supported("-maes");
                } else if target_arch == "aarch64" {
                    c_build
                        .flag_if_supported("-march=armv8-a+crypto")
                        .flag_if_supported("-flax-vector-conversions");
                    c_build.define("__ARM_NEON", None);
                }
            } else {
                add_msvc_includes(&mut c_build);
            }
            c_build.compile("verushash_c");

            // 2. Compile C++ sources (verus_hash, verus_clhash, ffi_wrapper)
            //    Also compile haraka C sources here so all symbols are in one archive.
            let mut cpp_build = cc::Build::new();
            cpp_build
                .cpp(true)
                .file("csrc/verushash/real/haraka.c")
                .file("csrc/verushash/real/haraka_portable.c")
                .file("csrc/verushash/real/verus_hash.cpp")
                .file("csrc/verushash/real/verus_clhash.cpp")
                .file("csrc/verushash/real/verus_clhash_portable.cpp")
                .file("csrc/verushash/real/ffi_wrapper_v3.cpp")
                .include("csrc/verushash/real")
                .opt_level(3)
                .warnings(false)
                .cargo_warnings(false)
                .flag_if_supported("-std=c++17")
                .flag_if_supported("-funroll-loops")
                .flag_if_supported("-fomit-frame-pointer")
                .flag_if_supported("-fPIC");
            if !is_msvc {
                if target_arch == "x86_64" {
                    cpp_build
                        .flag_if_supported("-mpclmul")
                        .flag_if_supported("-msse4")
                        .flag_if_supported("-msse4.1")
                        .flag_if_supported("-msse4.2")
                        .flag_if_supported("-mssse3")
                        .flag_if_supported("-maes");
                } else if target_arch == "aarch64" {
                    cpp_build
                        .flag_if_supported("-march=armv8-a+crypto")
                        .flag_if_supported("-flax-vector-conversions");
                    cpp_build.define("__ARM_NEON", None);
                }
                if target_os == "macos" {
                    cpp_build.flag_if_supported("-stdlib=libc++");
                }
            } else {
                add_msvc_includes(&mut cpp_build);
            }
            cpp_build.compile("verushash_cpp");

            // 3. Link C++ stdlib + force re-link C archive for unresolved symbols
            if target_os == "macos" {
                println!("cargo:rustc-link-lib=c++");
            } else if !is_msvc {
                println!("cargo:rustc-link-lib=stdc++");
            }
            let out_dir = env::var("OUT_DIR").unwrap_or_default();
            if !out_dir.is_empty() {
                println!("cargo:rustc-link-search=native={}", out_dir);
                println!("cargo:rustc-link-lib=static=verushash_c");
            }
            println!("cargo:rerun-if-changed=csrc/verushash/real/");
        } else {
            // --- Fallback: portable stub (Keccak-256 placeholder) ---
            base_build(
                "csrc/verushash/verushash_portable.c",
                "verushash_zion",
                &target_os,
                is_msvc,
            );
        }
    }

    // -----------------------------------------------------------------------
    // RandomX  (XMR, ZEPH)
    //   Portable stub; for production replace with a wrapper around the full
    //   Tevador/randomx C++ library (see algorithms/randomx/README.md).
    // -----------------------------------------------------------------------
    if feat("native-randomx") {
        base_build(
            "csrc/randomx/randomx_stub.c",
            "randomx_zion",
            &target_os,
            is_msvc,
        );
    }
}
