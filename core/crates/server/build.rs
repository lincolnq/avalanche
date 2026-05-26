fn main() {
    // Fall back to the vendored protoc if PROTOC is not set in the environment.
    if std::env::var("PROTOC").is_err() {
        let protoc = protoc_bin_vendored::protoc_bin_path().unwrap();
        std::env::set_var("PROTOC", protoc);
    }
    prost_build::compile_protos(&["../../proto/ws.proto"], &["../../proto"]).unwrap();
}
