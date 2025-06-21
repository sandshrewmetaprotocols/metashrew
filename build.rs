use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=metashrew-minimal");
    println!("cargo:rerun-if-changed=build.rs");
    
    // Get the output directory
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("metashrew_minimal.wasm");
    
    // Create the directory for the WASM file if it doesn't exist
    fs::create_dir_all(dest_path.parent().unwrap()).unwrap();
    
    // Check if metashrew-minimal directory exists
    let minimal_dir = Path::new("metashrew-minimal");
    if minimal_dir.exists() {
        // Build the metashrew-minimal program
        println!("Building metashrew-minimal WASM module...");
        
        let status = Command::new("cargo")
            .current_dir(minimal_dir)
            .args(&["build", "--target", "wasm32-unknown-unknown", "--release"])
            .status()
            .expect("Failed to build metashrew-minimal");
        
        if !status.success() {
            panic!("Failed to build metashrew-minimal");
        }
        
        // Copy the built WASM file to the output directory
        let src_wasm = Path::new("metashrew-minimal/target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
        if src_wasm.exists() {
            fs::copy(src_wasm, &dest_path).expect("Failed to copy WASM file");
            println!("Successfully built and copied metashrew-minimal WASM module");
        } else {
            panic!("Built WASM file not found at expected location");
        }
    } else {
        // If metashrew-minimal doesn't exist, use a placeholder
        println!("metashrew-minimal directory not found, using placeholder WASM module");
        
        // Create a minimal placeholder WASM file
        // This is just a placeholder and won't actually work as a WASM module
        fs::write(&dest_path, b"\0asm\x01\0\0\0").expect("Failed to write placeholder WASM file");
    }
    
    // Generate Rust code to include the WASM file
    let include_bytes_path = format!("include_bytes!(concat!(env!(\"OUT_DIR\"), \"/metashrew_minimal.wasm\"))");
    
    let gen_code = format!(
        "/// The metashrew-minimal WASM module as a byte array
pub const METASHREW_MINIMAL_WASM: &[u8] = {};
",
        include_bytes_path
    );
    
    // Write the generated code to a file
    let gen_path = Path::new(&out_dir).join("metashrew_minimal_wasm.rs");
    fs::write(&gen_path, gen_code).expect("Failed to write generated code");
    
    println!("cargo:rustc-env=METASHREW_MINIMAL_WASM_PATH={}", dest_path.display());
}