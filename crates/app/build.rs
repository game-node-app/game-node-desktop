use std::fmt::Write as _;
use std::path::PathBuf;

/// Embeds every `providers/*.toml` at the repository root so the app reads no manifest from disk
/// at runtime. The generated file lists `(file name, text)` pairs for the provider loader.
fn embed_provider_manifests() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let providers = manifest_dir.join("../../providers");
    println!("cargo:rerun-if-changed={}", providers.display());

    let mut files: Vec<PathBuf> = std::fs::read_dir(&providers)
        .expect("providers/ directory")
        .map(|entry| entry.expect("providers/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();

    let mut code = String::from("pub const BUNDLED_MANIFESTS: &[(&str, &str)] = &[\n");
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let name = file
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        let absolute = file.canonicalize().expect("canonical manifest path");
        writeln!(
            code,
            "    ({name:?}, include_str!({:?})),",
            absolute.to_string_lossy()
        )
        .expect("write to string");
    }
    code.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("providers.rs");
    std::fs::write(out, code).expect("write providers.rs");
}

fn main() {
    embed_windows_icon();
    println!("cargo:rerun-if-env-changed=GAMENODE_BUILD_VERSION");
    let version = std::env::var("GAMENODE_BUILD_VERSION")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").expect("package version"));
    assert!(
        !version.is_empty() && !version.contains(['\n', '\r']),
        "build version must be a nonempty single line"
    );
    println!("cargo:rustc-env=GAMENODE_BUILD_VERSION={version}");
    embed_provider_manifests();
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("../../i18n");
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint compilation failed");
}

fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=ui/icons/gamenode.ico");
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon("ui/icons/gamenode.ico");
    resources
        .compile_for("gamenode")
        .expect("failed to embed the Windows application icon");
}
