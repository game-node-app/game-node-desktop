fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("../../i18n");
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint compilation failed");
}
