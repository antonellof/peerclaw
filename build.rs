fn main() {
    // Link required frameworks on macOS (for llama.cpp GPU acceleration)
    #[cfg(target_os = "macos")]
    {
        // Metal frameworks for GPU
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=MetalKit");
        println!("cargo:rustc-link-lib=framework=MetalPerformanceShaders");

        // Accelerate framework for BLAS/vectorized math
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }
}
