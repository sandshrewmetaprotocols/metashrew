use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=crates/metashrew-minimal/src");
    println!("cargo:rerun-if-changed=crates/metashrew-minimal/Cargo.toml");

    let source_wasm = Path::new("target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    
    // Check if we're running tests or in a build context that might cause deadlock
    let is_testing = env::var("CARGO_CFG_TEST").is_ok() ||
                     env::var("CARGO_PRIMARY_PACKAGE").is_ok() ||
                     env::args().any(|arg| arg.contains("test"));
    
    if is_testing && source_wasm.exists() {
        // For tests, just use the existing WASM file to avoid deadlock
        println!("cargo:warning=Using existing WASM file for tests to avoid build deadlock");
        println!("cargo:rustc-env=METASHREW_MINIMAL_WASM={}", source_wasm.display());
        return;
    }
    
    let out_dir = env::var("OUT_DIR").unwrap();
    let wasm_path = Path::new(&out_dir).join("metashrew_minimal.wasm");

    // If WASM file exists, just copy it and set the env var
    if source_wasm.exists() {
        std::fs::copy(&source_wasm, &wasm_path)
            .expect("Failed to copy WASM file to output directory");
        println!("cargo:rustc-env=METASHREW_MINIMAL_WASM={}", wasm_path.display());
        return;
    }

    // Only build if WASM doesn't exist and we're not in a test context
    if !is_testing {
        let output = Command::new("cargo")
            .args(&[
                "build",
                "--release",
                "--target", "wasm32-unknown-unknown",
                "--package", "metashrew-minimal"
            ])
            .output()
            .expect("Failed to execute cargo build for metashrew-minimal");

        if !output.status.success() {
            panic!(
                "Failed to build metashrew-minimal:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Copy the built WASM file to our output directory
        std::fs::copy(&source_wasm, &wasm_path)
            .expect("Failed to copy WASM file to output directory");
        println!("cargo:rustc-env=METASHREW_MINIMAL_WASM={}", wasm_path.display());
    } else {
        // For tests without existing WASM, provide a warning and fallback
        println!("cargo:warning=WASM file not found and cannot build during tests. Please run: cargo build --target wasm32-unknown-unknown --release -p metashrew-minimal");
        println!("cargo:rustc-env=METASHREW_MINIMAL_WASM={}", source_wasm.display());
    }
}