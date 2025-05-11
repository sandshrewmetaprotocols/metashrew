use protobuf_codegen;
use std::process::Command;

fn main() {
    let mut codegen = protobuf_codegen::Codegen::new();
    codegen = codegen.out_dir("src/proto").inputs(&["proto/metashrew.proto"]).include("proto");
    
    // Check if protoc is available in the system path
    let protoc_available = Command::new("protoc").arg("--version").output().is_ok();
    
    if protoc_available {
        // Use system protoc if available (important for Android/Termux)
        println!("cargo:warning=Using system protoc");
        codegen = codegen.protoc();
    } else {
        // Fall back to vendored protoc binary, but handle potential errors
        println!("cargo:warning=Using vendored protoc");
        match protoc_bin_vendored::protoc_bin_path() {
            Ok(path) => {
                codegen = codegen.protoc().protoc_path(&path);
            },
            Err(e) => {
                // If vendored protoc is not available for this platform, try to use system protoc as a last resort
                println!("cargo:warning=Vendored protoc not available for this platform: {:?}", e);
                println!("cargo:warning=Attempting to use system protoc without explicit path");
                codegen = codegen.protoc();
            }
        }
    }
    
    codegen.run().expect("running protoc failed");
}
