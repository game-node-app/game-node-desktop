fn main() {
    println!("cargo:rerun-if-changed=proto/plugin.proto");
    tonic_prost_build::compile_protos("proto/plugin.proto")
        .expect("compile GameNode Plugin API protobuf");
}
