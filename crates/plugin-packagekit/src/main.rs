#[cfg(target_os = "linux")]
mod linux {
    use std::{collections::BTreeMap, env, fs, path::PathBuf, sync::Arc, time::Duration};

    use futures_util::StreamExt;
    use gamenode_plugin_protocol::{capability, proto::*};
    use gamenode_plugin_sdk::{Error, HostClient, Launch, ProviderPlugin, Session};
    use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

    const PLUGIN_ID: &str = "org.gamenode.plugin.packagekit-discovery";
    const PACKAGEKIT_SERVICE: &str = "org.freedesktop.PackageKit";
    const PACKAGEKIT_PATH: &str = "/org/freedesktop/PackageKit";
    const PACKAGEKIT_INTERFACE: &str = "org.freedesktop.PackageKit";
    const TRANSACTION_INTERFACE: &str = "org.freedesktop.PackageKit.Transaction";
    const FILTER_INSTALLED: u64 = 1;
    const MAX_DESKTOP_FILES: usize = 4_096;
    const DESKTOP_SEARCH_BATCH: usize = 128;

    pub(super) async fn run() -> Result<(), &'static str> {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .try_init();
        let launch = Launch::from_env().map_err(|_| "invalid_launch_configuration")?;
        let session = Session::open(
            launch,
            PLUGIN_ID,
            "packagekit",
            env!("CARGO_PKG_VERSION"),
            &[capability::CONTEXT, capability::DISCOVERY],
        )
        .await
        .map_err(|_| "provider_session_unavailable")?;
        gamenode_plugin_sdk::run(session, Arc::new(PackageManagerPlugin))
            .await
            .map_err(|_| "provider_session_failed")
    }

    struct PackageManagerPlugin;

    #[async_trait::async_trait]
    impl ProviderPlugin for PackageManagerPlugin {
        async fn handle(
            &self,
            command: ProviderCommand,
            host: HostClient,
        ) -> Result<OperationResult, Error> {
            let operation =
                ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
            match operation {
                ProviderOperation::Initialize => Ok(OperationResult {
                    disposition: OperationDisposition::Completed as i32,
                    ..Default::default()
                }),
                ProviderOperation::Discover => {
                    let context = host.provider_context().await?;
                    if context.provider_id != "packagekit" || context.platform != "linux" {
                        return Err(Error::Denied);
                    }
                    let candidates =
                        tokio::time::timeout(Duration::from_secs(60), scan_package_manager())
                            .await
                            .map_err(|_| Error::Unavailable)?
                            .map_err(|failure| {
                                tracing::warn!(failure, "Linux package discovery failed");
                                Error::Unavailable
                            })?;
                    Ok(OperationResult {
                        disposition: OperationDisposition::Completed as i32,
                        discovery: Some(DiscoveryResult {
                            provider_id: "packagekit".to_owned(),
                            records: candidates
                                .into_iter()
                                .map(|record| DiscoveryRecord {
                                    id: record.id,
                                    name: record.name.unwrap_or_default(),
                                    install_dir: record.install_dir,
                                    fields: record.fields.into_iter().collect(),
                                })
                                .collect(),
                        }),
                        ..Default::default()
                    })
                }
                _ => Err(Error::Denied),
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PackageBackend {
        Pacman,
        PackageKit,
    }

    impl PackageBackend {
        fn for_os_release(os_release: &str) -> Self {
            let mut id = "";
            let mut id_like = "";
            for line in os_release.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "ID" => id = value.trim_matches('"').trim_matches('\''),
                        "ID_LIKE" => id_like = value.trim_matches('"').trim_matches('\''),
                        _ => {}
                    }
                }
            }
            if id.eq_ignore_ascii_case("arch")
                || id_like
                    .split_ascii_whitespace()
                    .any(|family| family.eq_ignore_ascii_case("arch"))
            {
                Self::Pacman
            } else {
                Self::PackageKit
            }
        }
    }

    async fn scan_package_manager() -> Result<Vec<PackageCandidate>, &'static str> {
        let (os_release, desktop_entries) = tokio::task::spawn_blocking(|| {
            let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
            let desktop_entries = read_game_desktop_entries()?;
            Ok::<_, &'static str>((os_release, desktop_entries))
        })
        .await
        .map_err(|_| "desktop_metadata_failed")??;
        match PackageBackend::for_os_release(&os_release) {
            PackageBackend::Pacman => scan_pacman(desktop_entries).await,
            PackageBackend::PackageKit => scan_packagekit(desktop_entries).await,
        }
    }

    async fn scan_pacman(
        desktop_entries: Vec<DesktopEntry>,
    ) -> Result<Vec<PackageCandidate>, &'static str> {
        let mut records = BTreeMap::<String, PackageCandidate>::new();
        for entry in desktop_entries {
            let output = tokio::process::Command::new("pacman")
                .args(["-Qqo"])
                .arg(&entry.path)
                .env("LC_ALL", "C")
                .output()
                .await
                .map_err(|_| "pacman_unavailable")?;
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                if error.contains("No package owns") {
                    continue;
                }
                return Err("pacman_query_failed");
            }
            let Some(package_id) = parse_package_owner(&output.stdout)? else {
                continue;
            };
            let Some(candidate) = candidate_from_desktop(&package_id, &entry, "pacman") else {
                continue;
            };
            records.entry(candidate.id.clone()).or_insert(candidate);
        }
        Ok(records.into_values().collect())
    }

    async fn scan_packagekit(
        desktop_entries: Vec<DesktopEntry>,
    ) -> Result<Vec<PackageCandidate>, &'static str> {
        let connection = Connection::system()
            .await
            .map_err(|_| "packagekit_unavailable")?;
        let desktop_paths = desktop_entries
            .iter()
            .map(|entry| entry.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if desktop_paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut package_rows = Vec::new();
        for batch in desktop_paths.chunks(DESKTOP_SEARCH_BATCH) {
            package_rows.extend(
                transaction_packages(
                    &connection,
                    "SearchFiles",
                    &(FILTER_INSTALLED, batch.to_vec()),
                )
                .await
                .map_err(|_| "packagekit_query_failed")?,
            );
        }
        let by_path = desktop_entries
            .into_iter()
            .map(|entry| (entry.path.to_string_lossy().into_owned(), entry))
            .collect::<BTreeMap<_, _>>();
        let mut records = BTreeMap::<String, PackageCandidate>::new();
        for (package_id, summary) in package_rows {
            let Some(package_name) = normalized_package_name(&package_id) else {
                continue;
            };
            for entry in by_path
                .values()
                .filter(|entry| package_id_has_desktop(package_name, &entry.path, &entry.exec))
            {
                if let Some(binary) = resolve_exec(&entry.exec) {
                    records
                        .entry(package_name.to_owned())
                        .or_insert_with(|| PackageCandidate {
                            id: package_name.to_owned(),
                            name: entry
                                .name
                                .clone()
                                .or_else(|| (!summary.is_empty()).then_some(summary.clone())),
                            install_dir: binary
                                .parent()
                                .unwrap_or(&binary)
                                .to_string_lossy()
                                .into_owned(),
                            fields: BTreeMap::from([
                                ("package_id".to_owned(), package_id.clone()),
                                ("package_manager".to_owned(), "packagekit".to_owned()),
                                (
                                    "desktop_file".to_owned(),
                                    entry.path.to_string_lossy().into_owned(),
                                ),
                                (
                                    "executable".to_owned(),
                                    binary.to_string_lossy().into_owned(),
                                ),
                            ]),
                        });
                }
            }
        }
        Ok(records.into_values().collect())
    }

    async fn transaction_packages<B>(
        connection: &Connection,
        method: &str,
        body: &B,
    ) -> zbus::Result<Vec<(String, String)>>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let root = Proxy::new(
            connection,
            PACKAGEKIT_SERVICE,
            PACKAGEKIT_PATH,
            PACKAGEKIT_INTERFACE,
        )
        .await?;
        let transaction: OwnedObjectPath = root.call("CreateTransaction", &()).await?;
        let proxy = Proxy::new(
            connection,
            PACKAGEKIT_SERVICE,
            transaction,
            TRANSACTION_INTERFACE,
        )
        .await?;
        let mut packages = proxy.receive_signal("Package").await?;
        let mut finished = proxy.receive_signal("Finished").await?;
        proxy.call_method(method, body).await?;
        let mut result = Vec::new();
        loop {
            tokio::select! {
                biased;
                message = packages.next() => {
                    let Some(message) = message else { break };
                    if let Ok((_, package_id, summary)) = message.body().deserialize::<(u32, String, String)>() {
                        if package_id.contains(';') {
                            result.push((package_id, summary));
                        }
                    }
                }
                message = finished.next() => {
                    let Some(message) = message else { break };
                    let (exit, _runtime) = message.body().deserialize::<(u32, u32)>()?;
                    if exit != 1 { return Err(zbus::Error::Failure("PackageKit transaction failed".to_owned())); }
                    break;
                }
            }
        }
        Ok(result)
    }

    struct DesktopEntry {
        path: PathBuf,
        name: Option<String>,
        exec: String,
    }

    fn read_game_desktop_entries() -> Result<Vec<DesktopEntry>, &'static str> {
        let directories = ["/usr/share/applications", "/usr/local/share/applications"];
        let mut entries = Vec::new();
        for directory in directories {
            let Ok(files) = fs::read_dir(directory) else {
                continue;
            };
            for file in files
                .flatten()
                .take(MAX_DESKTOP_FILES.saturating_sub(entries.len()))
            {
                if !file
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "desktop")
                {
                    continue;
                }
                let Ok(text) = fs::read_to_string(file.path()) else {
                    continue;
                };
                let Some(entry) = parse_game_desktop_entry(file.path(), &text) else {
                    continue;
                };
                entries.push(entry);
                if entries.len() >= MAX_DESKTOP_FILES {
                    break;
                }
            }
        }
        Ok(entries)
    }

    fn parse_game_desktop_entry(path: PathBuf, text: &str) -> Option<DesktopEntry> {
        let mut in_entry = false;
        let mut entry_type = None;
        let mut name = None;
        let mut exec = None;
        let mut categories = String::new();
        let mut no_display = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Type" => entry_type = Some(value),
                    "Name" => name = Some(value.to_owned()),
                    "Exec" => exec = Some(value.to_owned()),
                    "Categories" => categories = value.to_owned(),
                    "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                    _ => {}
                }
            }
        }
        let is_game = categories
            .split(';')
            .any(|category| category.eq_ignore_ascii_case("game"));
        if entry_type != Some("Application") || !is_game || no_display {
            return None;
        }
        Some(DesktopEntry {
            path,
            name,
            exec: exec?,
        })
    }

    fn package_id_has_desktop(
        package_name: &str,
        desktop_path: &std::path::Path,
        exec: &str,
    ) -> bool {
        let desktop_stem = desktop_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let executable = exec
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches('"');
        let executable = std::path::Path::new(executable)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        desktop_stem == package_name
            || desktop_stem
                .strip_prefix(package_name)
                .is_some_and(|suffix| {
                    suffix.starts_with('-') || suffix.chars().all(|ch| ch.is_ascii_digit())
                })
            || executable == package_name
            || executable
                .strip_prefix(package_name)
                .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
    }

    fn resolve_exec(exec: &str) -> Option<PathBuf> {
        let executable = exec.split_whitespace().next()?.trim_matches('"');
        if executable.is_empty() || executable.starts_with('%') || executable.contains('$') {
            return None;
        }
        let candidate = PathBuf::from(executable);
        if candidate.is_absolute() && candidate.is_file() {
            return Some(candidate);
        }
        env::var_os("PATH")?
            .to_string_lossy()
            .split(':')
            .map(|directory| PathBuf::from(directory).join(executable))
            .find(|path| path.is_file())
    }

    struct PackageCandidate {
        id: String,
        name: Option<String>,
        install_dir: String,
        fields: BTreeMap<String, String>,
    }

    fn parse_package_owner(output: &[u8]) -> Result<Option<String>, &'static str> {
        let package_id = String::from_utf8(output.to_vec())
            .map_err(|_| "pacman_invalid_output")?
            .trim()
            .to_owned();
        if package_id.is_empty()
            || package_id.len() > 256
            || package_id.chars().any(char::is_control)
        {
            return Ok(None);
        }
        Ok(Some(package_id))
    }

    fn normalized_package_name(package_id: &str) -> Option<&str> {
        let name = package_id.split(';').next()?.trim();
        (!name.is_empty()
            && name.len() <= 256
            && !name.chars().any(char::is_control)
            && !name.contains('/'))
        .then_some(name)
    }

    fn candidate_from_desktop(
        package_id: &str,
        entry: &DesktopEntry,
        package_manager: &str,
    ) -> Option<PackageCandidate> {
        let binary = resolve_exec(&entry.exec)?;
        Some(PackageCandidate {
            id: package_id.to_owned(),
            name: entry.name.clone(),
            install_dir: binary
                .parent()
                .unwrap_or(&binary)
                .to_string_lossy()
                .into_owned(),
            fields: BTreeMap::from([
                ("package_id".to_owned(), package_id.to_owned()),
                (
                    "desktop_file".to_owned(),
                    entry.path.to_string_lossy().into_owned(),
                ),
                (
                    "executable".to_owned(),
                    binary.to_string_lossy().into_owned(),
                ),
                ("package_manager".to_owned(), package_manager.to_owned()),
            ]),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{
            DesktopEntry, PackageBackend, candidate_from_desktop, normalized_package_name,
            parse_game_desktop_entry, parse_package_owner,
        };
        use std::{collections::BTreeMap, path::PathBuf};

        #[test]
        fn selects_pacman_for_arch_and_arch_derivatives() {
            assert_eq!(
                PackageBackend::for_os_release("ID=arch\n"),
                PackageBackend::Pacman
            );
            assert_eq!(
                PackageBackend::for_os_release("ID=manjaro\nID_LIKE=arch\n"),
                PackageBackend::Pacman
            );
        }

        #[test]
        fn selects_packagekit_for_other_or_unknown_distributions() {
            assert_eq!(
                PackageBackend::for_os_release("ID=fedora\nID_LIKE=redhat\n"),
                PackageBackend::PackageKit
            );
            assert_eq!(
                PackageBackend::for_os_release(""),
                PackageBackend::PackageKit
            );
        }

        #[test]
        fn pacman_owner_output_accepts_package_and_skips_empty_or_malformed_values() {
            assert_eq!(
                parse_package_owner(b"supertux\n").unwrap(),
                Some("supertux".to_owned())
            );
            assert_eq!(parse_package_owner(b"\n").unwrap(), None);
            assert_eq!(parse_package_owner(b"bad\nname\n").unwrap(), None);
            assert_eq!(parse_package_owner(&[0xff]), Err("pacman_invalid_output"));
        }

        #[test]
        fn packagekit_ids_normalize_to_the_same_package_name_as_pacman() {
            assert_eq!(
                normalized_package_name("supertux;0.6.3-4;x86_64;installed"),
                Some("supertux")
            );
            assert_eq!(normalized_package_name("supertux"), Some("supertux"));
            assert_eq!(normalized_package_name(";1.0;x86_64;installed"), None);
            assert_eq!(normalized_package_name("bad/name;1.0;arch;installed"), None);
        }

        #[test]
        fn game_desktop_metadata_is_required_and_candidates_keep_package_identity() {
            let desktop = parse_game_desktop_entry(
                PathBuf::from("/usr/share/applications/supertux2.desktop"),
                "[Desktop Entry]\nType=Application\nName=SuperTux\nExec=/usr/bin/true\nCategories=Game;\n",
            )
            .unwrap();
            let candidate = candidate_from_desktop("supertux", &desktop, "pacman").unwrap();
            assert_eq!(candidate.id, "supertux");
            assert_eq!(candidate.name.as_deref(), Some("SuperTux"));
            assert_eq!(candidate.fields["package_manager"], "pacman");

            assert!(parse_game_desktop_entry(
                PathBuf::from("/usr/share/applications/not-a-game.desktop"),
                "[Desktop Entry]\nType=Application\nName=Text Editor\nExec=/usr/bin/true\nCategories=Utility;\n",
            )
            .is_none());
            assert!(parse_game_desktop_entry(
                PathBuf::from("/usr/share/applications/hidden.desktop"),
                "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/true\nCategories=Game;\nNoDisplay=true\n",
            )
            .is_none());
            assert!(
                parse_game_desktop_entry(
                    PathBuf::from("/usr/share/applications/malformed.desktop"),
                    "not a desktop entry",
                )
                .is_none()
            );
        }

        #[test]
        fn repeated_desktop_candidates_share_a_stable_package_id() {
            let first = DesktopEntry {
                path: PathBuf::from("/usr/share/applications/supertux2.desktop"),
                name: Some("SuperTux".to_owned()),
                exec: "/usr/bin/true".to_owned(),
            };
            let second = DesktopEntry {
                path: PathBuf::from("/usr/share/applications/supertux-editor.desktop"),
                name: Some("SuperTux Editor".to_owned()),
                exec: "/usr/bin/true".to_owned(),
            };
            let mut candidates = BTreeMap::new();
            candidates
                .entry("supertux".to_owned())
                .or_insert_with(|| candidate_from_desktop("supertux", &first, "pacman").unwrap());
            candidates
                .entry("supertux".to_owned())
                .or_insert_with(|| candidate_from_desktop("supertux", &second, "pacman").unwrap());
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates["supertux"].name.as_deref(), Some("SuperTux"));
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    if let Err(code) = linux::run().await {
        tracing::warn!(failure = code, "Linux package manager plugin failed");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
