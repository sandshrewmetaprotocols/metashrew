use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let status = Command::new("cargo")
        .args(&[
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .status()
        .expect("failed to build wasm");

    if !status.success() {
        panic!("failed to build wasm");
    }
}