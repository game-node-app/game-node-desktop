fn main() {
    println!("cargo:rerun-if-changed=proto/plugin.proto");
    let mut config = prost_build::Config::new();
    // Build scripts execute on the build host, including during cross-compiles.
    // Do not mutate the process environment to select the compiler.
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path().expect("bundled protoc"));
    let protobuf_include = protoc_bin_vendored::include_path().expect("bundled protobuf includes");
    let descriptor_path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("plugin_descriptor.bin");
    let includes = [
        "proto",
        protobuf_include
            .to_str()
            .expect("protobuf include path is UTF-8"),
    ];
    tonic_prost_build::configure()
        .file_descriptor_set_path(descriptor_path)
        .skip_debug([
            ".gamenode.plugin.v1.OpenSessionRequest",
            ".gamenode.plugin.v1.SubmitDiscoveryRequest",
            ".gamenode.plugin.v1.ServiceContext",
            ".gamenode.plugin.v1.SecretRequest",
            ".gamenode.plugin.v1.SecretReply",
            ".gamenode.plugin.v1.LoginCookie",
            ".gamenode.plugin.v1.LoginReply",
            ".gamenode.plugin.v1.SettingsValue",
        ])
        .compile_with_config(config, &["proto/plugin.proto"], &includes)
        .expect("compile GameNode Plugin API protobuf");
}
