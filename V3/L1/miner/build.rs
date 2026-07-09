fn main() {
    #[cfg(feature = "gpu-opencl")]
    {
        // Provide OpenCL.lib location for linking on Windows
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = std::path::Path::new(&manifest_dir);

        // V3/target/ — primary location
        if let Some(workspace_target) = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("target"))
        {
            println!(
                "cargo:rustc-link-search=native={}",
                workspace_target.display()
            );
        }

        // opencl_sdk/ — repo-bundled SDK
        if let Some(opencl_sdk) = manifest_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("opencl_sdk"))
        {
            if opencl_sdk.exists() {
                println!("cargo:rustc-link-search=native={}", opencl_sdk.display());
            }
        }

        // L1/native-libs/ — legacy native libs
        if let Some(native_libs) = manifest_path.parent().map(|p| p.join("native-libs")) {
            if native_libs.exists() {
                println!("cargo:rustc-link-search=native={}", native_libs.display());
            }
        }
    }

    #[cfg(feature = "gpu-metal")]
    {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
