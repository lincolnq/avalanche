fn main() {
    // If PROTOC is not set in the environment, fall back to the vendored binary
    // so that builds work without a system-wide protoc installation.
    if std::env::var("PROTOC").is_err() {
        let protoc = protoc_bin_vendored::protoc_bin_path().unwrap();
        std::env::set_var("PROTOC", protoc);
    }
    prost_build::compile_protos(&["../../proto/content.proto"], &["../../proto"]).unwrap();
}
