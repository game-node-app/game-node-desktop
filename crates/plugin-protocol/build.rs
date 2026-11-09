fn main() {
    println!("cargo:rerun-if-changed=proto/plugin.proto");
    let mut config = prost_build::Config::new();
    // Build scripts execute on the build host, including during cross-compiles.
    // Do not mutate the process environment to select the compiler.
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path().expect("bundled protoc"));
    tonic_prost_build::configure()
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
        .compile_with_config(config, &["proto/plugin.proto"], &["proto"])
        .expect("compile GameNode Plugin API protobuf");
}
