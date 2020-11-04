extern crate protoc_rust;

fn main() {
    protoc_rust::Codegen::new()
        .out_dir("src/protos")
        .inputs(&["proto/components.proto"])
        .run()
        .expect("protoc");
}
