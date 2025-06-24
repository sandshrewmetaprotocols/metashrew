use protobuf_codegen;

fn main() {
    protobuf_codegen::Codegen::new()
        // Use pure Rust parser instead of protoc for cross-platform compatibility
        .pure()
        .out_dir("src/proto")
        .inputs(&["proto/metashrew.proto"])
        .include("proto")
        .run()
        .expect("running protobuf codegen failed");
}
