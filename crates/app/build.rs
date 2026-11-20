use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Embeds every `providers/*.toml` at the repository root so the app reads no manifest from disk
/// at runtime. The generated file lists `(file name, text)` pairs for the provider loader.
fn embed_provider_manifests(repo_root: &Path) {
    let providers = repo_root.join("providers");
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

/// Reads the repo-relative login-policy reference from a trusted descriptor. Descriptors use a
/// single `login_policy = "..."` key, and this keeps the build script free of a TOML dependency.
fn login_policy_reference(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("login_policy")?;
        let rest = rest.trim_start().strip_prefix('=')?.trim();
        let value = rest
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                rest.strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })?;
        Some(value.replace('\\', "/"))
    })
}

/// Embeds `plugins/*.toml` and every login policy they reference, exactly like provider
/// manifests. Descriptors and policies are compiled-in trusted data; nothing is read at runtime.
fn embed_plugin_descriptors(repo_root: &Path) {
    let plugins = repo_root.join("plugins");
    println!("cargo:rerun-if-changed={}", plugins.display());
    let canonical_repo = repo_root.canonicalize().expect("canonical repository root");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&plugins)
        .expect("plugins/ directory")
        .map(|entry| entry.expect("plugins/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();

    let mut descriptors =
        String::from("pub const BUNDLED_PLUGIN_DESCRIPTORS: &[(&str, &str)] = &[\n");
    let mut policies: Vec<(String, PathBuf)> = Vec::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let text = std::fs::read_to_string(&file).expect("read plugin descriptor");
        if let Some(reference) = login_policy_reference(&text) {
            let path = repo_root.join(&reference);
            println!("cargo:rerun-if-changed={}", path.display());
            let absolute = path
                .canonicalize()
                .expect("login policy referenced by a plugin descriptor");
            assert!(
                absolute.starts_with(&canonical_repo),
                "login policy must stay inside the repository"
            );
            if !policies.iter().any(|(name, _)| name == &reference) {
                policies.push((reference, absolute));
            }
        }
        let name = file
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        let absolute = file.canonicalize().expect("canonical plugin path");
        writeln!(
            descriptors,
            "    ({name:?}, include_str!({:?})),",
            absolute.to_string_lossy()
        )
        .expect("write to string");
    }
    descriptors.push_str("];\n");

    policies.sort_by(|left, right| left.0.cmp(&right.0));
    let mut policy_code = String::from("pub const BUNDLED_LOGIN_POLICIES: &[(&str, &str)] = &[\n");
    for (name, path) in policies {
        writeln!(
            policy_code,
            "    ({name:?}, include_str!({:?})),",
            path.to_string_lossy()
        )
        .expect("write to string");
    }
    policy_code.push_str("];\n");
    descriptors.push_str(&policy_code);

    let out =
        PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("plugin_descriptors.rs");
    std::fs::write(out, descriptors).expect("write plugin_descriptors.rs");
}

/// Formats a Unix timestamp as a UTC `YYYY-MM-DD` date. Build scripts cannot depend on a date
/// crate, so this uses the civil-from-days algorithm (valid for the Gregorian calendar).
fn utc_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn main() {
    println!("cargo:rerun-if-env-changed=GAMENODE_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let version = std::env::var("GAMENODE_BUILD_VERSION")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").expect("package version"));
    assert!(
        !version.is_empty() && !version.contains(['\n', '\r']),
        "build version must be a nonempty single line"
    );
    println!("cargo:rustc-env=GAMENODE_BUILD_VERSION={version}");

    let compile_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let build_date = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|seconds| seconds.trim().parse::<i64>().ok())
        .map(utc_date)
        .unwrap_or_else(|| utc_date(compile_time));
    println!("cargo:rustc-env=GAMENODE_BUILD_DATE={build_date}");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo_root = manifest_dir.join("../..");
    embed_provider_manifests(&repo_root);
    embed_plugin_descriptors(&repo_root);
    tauri_build::build();
}
