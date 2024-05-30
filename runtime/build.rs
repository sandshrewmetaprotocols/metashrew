use protobuf_codegen;
fn main() {
    protobuf_codegen::Codegen::new().out_dir("src/proto").inputs(&["proto/metashrew.proto"]).include("proto").run().expect("running protoc failed");
}
