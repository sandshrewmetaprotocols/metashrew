use protobuf_codegen;
use protoc_bin_vendored;
use std::process::Command;
use std::path::Path;

fn main() {
    let mut binding = protobuf_codegen::Codegen::new();
    let mut codegen = binding.protoc();
    
    // Try to use vendored protoc binary first
    match protoc_bin_vendored::protoc_bin_path() {
        Ok(path) => {
            println!("Using vendored protoc binary");
            codegen = codegen.protoc_path(&path);
        },
        Err(e) => {
            // If vendored protoc is not available for this platform, try to use system protoc
            println!("Vendored protoc not available: {:?}", e);
            println!("Trying to use system protoc");
            
            // Check if protoc is available in PATH
            let protoc_check = Command::new("protoc")
                .arg("--version")
                .output();
                
            if protoc_check.is_err() {
                panic!("Could not find protoc in PATH and vendored protoc is not available for this platform ({}). Please install protoc manually.", e);
            }
            
            // Use system protoc
            codegen = codegen.protoc_path(Path::new("protoc"));
        }
    }
    
    // Continue with the rest of the build
    codegen
        .out_dir("src/proto")
        .inputs(&["proto/metashrew.proto"])
        .include("proto")
        .run()
        .expect("running protoc failed");
}
