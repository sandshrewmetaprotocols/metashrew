use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=crates/metashrew-minimal/src");
    println!("cargo:rerun-if-changed=crates/metashrew-minimal/Cargo.toml");

    let out_dir = env::var("OUT_DIR").unwrap();
    let wasm_path = Path::new(&out_dir).join("metashrew_minimal.wasm");

    // Build metashrew-minimal for wasm32-unknown-unknown target
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
    let source_wasm = Path::new("target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    std::fs::copy(&source_wasm, &wasm_path)
        .expect("Failed to copy WASM file to output directory");

    println!("cargo:rustc-env=METASHREW_MINIMAL_WASM={}", wasm_path.display());
}