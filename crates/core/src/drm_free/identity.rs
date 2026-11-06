//! Conservative identity inference. Similarity alone never authorizes discovery or a match.
use super::{Evidence, EvidenceKind};
use crate::game_api::GameSummary;
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

pub fn normalized(title: &str) -> String {
    title
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '’' | '‘' => '\'',
            '–' | '—' => '-',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn launcher(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("launcher") || name.contains("bootstrap")
}
pub fn maintenance(name: &str) -> bool {
    let lower = name.to_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    [
        "setup",
        "install",
        "unins",
        "update",
        "redist",
        "vcredist",
        "dxsetup",
        "support",
        "unitycrashhandler",
        "crashreport",
        "crashhandler",
        "crashpad",
        "ue4prereq",
        "ue5prereq",
    ]
    .iter()
    .any(|word| lower.starts_with(word))
        || [
            "editor",
            "config",
            "settings",
            "benchmark",
            "dedicatedserver",
            "crashreporter",
            "updater",
        ]
        .iter()
        .any(|word| lower.ends_with(word))
}
pub fn meaningful(title: &str) -> bool {
    let mut normalized = normalized(title);
    if let Some(stem) = normalized.strip_suffix(".exe") {
        normalized = stem.to_owned();
    }
    for suffix in [
        "-win64-shipping",
        "-win32-shipping",
        "-shipping",
        "_x64",
        "_x86",
    ] {
        if let Some(stem) = normalized.strip_suffix(suffix) {
            normalized = stem.to_owned();
        }
    }
    !normalized.is_empty()
        && !maintenance(&normalized)
        && !launcher(&normalized)
        && ![
            "game",
            "launcher",
            "bootstrap",
            "shipping",
            "win64",
            "win32",
            "unity",
            "unreal engine",
            "ue4game",
            "ue5game",
        ]
        .contains(&normalized.as_str())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleIdentity {
    pub title: String,
    pub strong: bool,
    pub conflict: bool,
}
pub fn infer(evidence: &[Evidence]) -> Option<TitleIdentity> {
    let rank = |kind: &EvidenceKind| match kind {
        EvidenceKind::Product => 0,
        EvidenceKind::Description => 1,
        EvidenceKind::Executable => 2,
        EvidenceKind::Manifest => 3,
        EvidenceKind::Folder => 4,
        EvidenceKind::ExternalId => 5,
    };
    let mut usable: Vec<_> = evidence
        .iter()
        .filter(|e| e.kind != EvidenceKind::ExternalId && meaningful(&e.value))
        .collect();
    usable.sort_by_key(|e| rank(&e.kind));
    let first = usable.first()?;
    let normalized = normalized(&first.value);
    let corroborated = usable
        .iter()
        .any(|e| e.kind != first.kind && self::normalized(&e.value) == normalized);
    let conflict = usable.iter().any(|e| {
        (e.strong || matches!(e.kind, EvidenceKind::Product | EvidenceKind::Description))
            && self::normalized(&e.value) != normalized
    });
    Some(TitleIdentity {
        title: first.value.clone(),
        strong: !conflict
            && (first.strong || corroborated || first.kind == EvidenceKind::Executable),
        conflict,
    })
}
#[derive(Clone, Debug)]
pub struct ManifestIdentity {
    pub title: String,
    pub aliases: BTreeSet<String>,
    pub installs: BTreeSet<String>,
    pub launch_hints: BTreeSet<String>,
    pub steam: Option<String>,
    pub gog: Option<String>,
    pub definition: serde_yaml::Value,
}
#[derive(Clone, Debug, Default)]
pub struct ManifestIndex {
    pub games: BTreeMap<String, ManifestIdentity>,
}
impl ManifestIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("manifest exceeds size limit".into());
        }
        let value: serde_yaml::Value = serde_yaml::from_slice(bytes).map_err(|e| e.to_string())?;
        let root = value.as_mapping().ok_or("manifest must be a mapping")?;
        if root.len() > 100_000 {
            return Err("too many manifest entries".into());
        }
        let mut index = Self::default();
        for (title, definition) in root {
            let title = title.as_str().ok_or("invalid title")?;
            if title.len() > 4096 || !definition.is_mapping() {
                return Err("invalid manifest entry".into());
            }
            let installs = definition
                .get("installDir")
                .and_then(|v| v.as_mapping())
                .map(|v| {
                    v.keys()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let launch_hints = definition
                .get("launch")
                .and_then(|v| v.as_mapping())
                .map(|v| {
                    v.keys()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let id = |key| {
                definition
                    .get(key)
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_u64())
                    .map(|n| n.to_string())
            };
            index.games.insert(
                title.into(),
                ManifestIdentity {
                    title: title.into(),
                    aliases: BTreeSet::new(),
                    installs,
                    launch_hints,
                    steam: id("steam"),
                    gog: id("gog"),
                    definition: definition.clone(),
                },
            );
        }
        for (title, definition) in root {
            let Some(mut target) = definition.get("alias").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut seen = BTreeSet::new();
            for _ in 0..32 {
                if !seen.insert(target) {
                    return Err("manifest alias cycle".into());
                }
                let destination = root
                    .get(serde_yaml::Value::String(target.into()))
                    .ok_or("unknown alias target")?;
                if let Some(next) = destination.get("alias").and_then(|v| v.as_str()) {
                    target = next;
                    continue;
                }
                let game = index.games.get_mut(target).ok_or("unknown alias target")?;
                if game.aliases.len() >= 1024 {
                    return Err("too many aliases".into());
                }
                game.aliases.insert(title.as_str().unwrap().into());
                break;
            }
            if seen.len() >= 32 {
                return Err("manifest alias chain exceeds limit".into());
            }
        }
        index
            .games
            .retain(|_, game| game.definition.get("alias").is_none());
        Ok(index)
    }
    /// A folder hint alone never upgrades an unidentified executable.
    pub fn corroborate(
        &self,
        evidence: &[Evidence],
        folder: &str,
        relative_executable: &str,
    ) -> Vec<Evidence> {
        let mut result = evidence.to_vec();
        let executable = relative_executable.replace('\\', "/");
        let executable_name = executable.rsplit('/').next().unwrap_or(&executable);
        let stem = executable_name
            .strip_suffix(".exe")
            .unwrap_or(executable_name);
        if maintenance(stem) || launcher(stem) {
            return result;
        }
        let local = infer(evidence);
        if local.as_ref().is_some_and(|identity| identity.conflict) {
            return result;
        }
        let matches: Vec<_> = self
            .games
            .values()
            .filter(|game| {
                let identity_names = std::iter::once(&game.title).chain(game.aliases.iter());
                let named_executable = meaningful(stem)
                    && identity_names
                        .clone()
                        .any(|name| normalized(name) == normalized(stem));
                let launch = game.launch_hints.iter().any(|hint| {
                    let hint = hint.replace('\\', "/");
                    let hint = hint.strip_prefix("<base>/").unwrap_or(&hint);
                    !hint.contains(['*', '?', '<', '>']) && hint.eq_ignore_ascii_case(&executable)
                });
                let product = evidence.iter().any(|e| {
                    matches!(e.kind, EvidenceKind::Product | EvidenceKind::Description)
                        && meaningful(&e.value)
                        && identity_names
                            .clone()
                            .any(|name| normalized(name) == normalized(&e.value))
                });
                let folder_hint = game
                    .installs
                    .iter()
                    .any(|name| normalized(name) == normalized(folder));
                launch || product || (folder_hint && named_executable)
            })
            .collect();
        if let [game] = matches.as_slice() {
            result.push(Evidence {
                kind: EvidenceKind::Manifest,
                value: game.title.clone(),
                strong: true,
            });
        }
        result
    }

    pub fn exact(&self, title: &str) -> Vec<&ManifestIdentity> {
        let normalized = normalized(title);
        self.games
            .values()
            .filter(|g| {
                self::normalized(&g.title) == normalized
                    || g.aliases.iter().any(|a| self::normalized(a) == normalized)
            })
            .collect()
    }
}
pub fn unique_exact(
    identity: &TitleIdentity,
    games: &[GameSummary],
    complete: bool,
) -> Option<u64> {
    if !identity.strong || identity.conflict || !complete || games.len() >= 20 {
        return None;
    }
    let ids: BTreeSet<_> = games
        .iter()
        .filter(|g| normalized(&g.name) == normalized(&identity.title))
        .map(|g| g.id)
        .collect();
    (ids.len() == 1).then(|| *ids.first().unwrap())
}

pub fn profile(
    game: &ManifestIdentity,
    installation: &super::Registration,
    hash: &str,
    id: String,
) -> Result<super::SaveProfile, String> {
    Ok(super::SaveProfile {
        id,
        installation: installation.id.clone(),
        revision: 1,
        title: game.title.clone(),
        manifest_hash: hash.into(),
        definition: serde_yaml::to_string(&game.definition).map_err(|e| e.to_string())?,
        files: vec![],
        registry: vec![],
        support: super::SaveSupport::Supported,
        automatic: None,
    })
}

pub fn preferred_target(
    root: &str,
    targets: &[String],
    manifest_hints: &[String],
) -> Option<String> {
    if targets.is_empty() {
        return None;
    }
    let normalized_root = root.replace('\\', "/").trim_end_matches('/').to_owned();
    let score = |target: &String| {
        let normalized_target = target.replace('\\', "/");
        let relative = if !normalized_root.is_empty()
            && normalized_target.len() > normalized_root.len()
            && normalized_target
                .get(..normalized_root.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&normalized_root))
            && normalized_target.as_bytes().get(normalized_root.len()) == Some(&b'/')
        {
            normalized_target[normalized_root.len() + 1..].trim_start_matches('/')
        } else {
            normalized_target.trim_start_matches('/')
        };
        let filename = relative.rsplit('/').next().unwrap_or(relative);
        let lower_rel = relative.to_lowercase();

        let mut manifest_rank = 2u8;
        for hint in manifest_hints {
            let hint = hint.replace('\\', "/");
            let hint = hint
                .strip_prefix("<base>/")
                .unwrap_or(&hint)
                .trim_start_matches('/');
            if hint.is_empty() || hint.contains(['*', '?', '<', '>']) {
                continue;
            }
            if hint.eq_ignore_ascii_case(relative) {
                manifest_rank = 0;
                break;
            }
            if !hint.contains('/') && hint.eq_ignore_ascii_case(filename) {
                manifest_rank = manifest_rank.min(1);
            }
        }

        let role_rank = match (
            maintenance(filename),
            launcher(filename),
            launcher(relative),
        ) {
            (false, false, false) => 0u8,
            (false, false, true) => 1u8,
            (false, true, _) => 2u8,
            (true, _, _) => 3u8,
        };

        let is_64 = ["x64", "win64", "amd64", "_64", "-64", "64.exe"]
            .iter()
            .any(|m| lower_rel.contains(m));
        let is_32 = ["x86", "win32", "_32", "-32", "32.exe"]
            .iter()
            .any(|m| lower_rel.contains(m));
        let arch_rank = if is_64 {
            0u8
        } else if is_32 {
            2u8
        } else {
            1u8
        };

        let depth = relative.bytes().filter(|&b| b == b'/').count();

        (
            manifest_rank,
            role_rank,
            arch_rank,
            depth,
            lower_rel,
            target.clone(),
        )
    };
    targets.iter().min_by_key(|t| score(t)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editions_and_sequels_stay_distinct_and_conflicts_block() {
        assert_ne!(normalized("Portal"), normalized("Portal 2"));
        assert_ne!(normalized("Game"), normalized("Game Remastered"));
        assert!(!meaningful("UnityCrashHandler64.exe"));
        let e = vec![
            Evidence {
                kind: EvidenceKind::Product,
                value: "Portal".into(),
                strong: true,
            },
            Evidence {
                kind: EvidenceKind::Manifest,
                value: "Portal 2".into(),
                strong: true,
            },
        ];
        assert!(infer(&e).unwrap().conflict);
        let i = TitleIdentity {
            title: "Portal".into(),
            strong: true,
            conflict: false,
        };
        let g = GameSummary {
            id: 1,
            name: "Portal".into(),
            release_year: None,
            cover_url: None,
        };
        assert_eq!(unique_exact(&i, std::slice::from_ref(&g), true), Some(1));
        assert_eq!(unique_exact(&i, &[g], false), None);
    }
    #[test]
    fn generic_shipping_and_engine_names_do_not_establish_identity() {
        for name in [
            "Game-Win64-Shipping.exe",
            "Launcher.exe",
            "Unity.exe",
            "UE4Game-Win64-Shipping.exe",
            "setup.exe",
        ] {
            assert!(!meaningful(name), "{name}");
        }
        let executable = Evidence {
            kind: EvidenceKind::Executable,
            value: "Celeste".into(),
            strong: false,
        };
        assert!(infer(&[executable]).unwrap().strong);
        let product = Evidence {
            kind: EvidenceKind::Product,
            value: "Celeste".into(),
            strong: false,
        };
        let conflicting = Evidence {
            kind: EvidenceKind::Description,
            value: "Another game".into(),
            strong: false,
        };
        assert!(infer(&[product, conflicting]).unwrap().conflict);
    }

    #[test]
    fn aliases_are_bounded_and_unknown_fields_tolerated() {
        let index =
            ManifestIndex::parse(b"Game:\n  files: {}\n  future: true\nOld:\n  alias: Game\n")
                .unwrap();
        assert_eq!(index.exact("Old")[0].title, "Game");
        assert!(ManifestIndex::parse(b"A:\n  alias: B\nB:\n  alias: A\n").is_err());
    }
    #[test]
    fn manifest_hints_and_aliases_do_not_turn_folder_only_evidence_into_identity() {
        let index = ManifestIndex::parse(b"Celeste:\n  installDir: {Renamed: {}}\n  launch: {Celeste.exe: {}}\n  steam: {id: 504230}\n  futureField: true\nOther edition:\n  files: {}\nOld Name:\n  alias: Celeste\n").unwrap();
        let game = index.exact("Old Name")[0];
        assert_eq!(game.steam.as_deref(), Some("504230"));
        assert!(game.installs.contains("Renamed"));
        assert!(game.launch_hints.contains("Celeste.exe"));
        assert!(index.exact("Other edition")[0].steam.is_none());
        let evidence = [Evidence {
            kind: EvidenceKind::Folder,
            value: "Celeste".into(),
            strong: false,
        }];
        assert!(!infer(&evidence).unwrap().strong);
        let mut aliases = String::from("Target: {}\n");
        for n in 0..1025 {
            aliases.push_str(&format!("Alias{n}: {{alias: Target}}\n"));
        }
        assert!(ManifestIndex::parse(aliases.as_bytes()).is_err());
    }

    #[test]
    fn preferred_target_ranks_manifest_hints_gameplay_64bit_and_shallowest_depth() {
        let root = r"C:\Games\Hollow Knight";
        assert_eq!(preferred_target(root, &[], &[]), None);

        let single = vec![r"C:\Games\Hollow Knight\hollow_knight.exe".to_owned()];
        assert_eq!(
            preferred_target(root, &single, &[]),
            Some(r"C:\Games\Hollow Knight\hollow_knight.exe".to_owned())
        );

        // 64-bit over neutral and 32-bit
        let multi_arch = vec![
            r"C:\Games\Hollow Knight\hollow_knight_x86.exe".to_owned(),
            r"C:\Games\Hollow Knight\hollow_knight.exe".to_owned(),
            r"C:\Games\Hollow Knight\hollow_knight_x64.exe".to_owned(),
        ];
        assert_eq!(
            preferred_target(root, &multi_arch, &[]),
            Some(r"C:\Games\Hollow Knight\hollow_knight_x64.exe".to_owned())
        );

        let arch_folders = vec![
            r"C:\Games\Celeste\x86\Celeste.exe".to_owned(),
            r"C:\Games\Celeste\x64\Celeste.exe".to_owned(),
        ];
        assert_eq!(
            preferred_target(r"C:\Games\Celeste", &arch_folders, &[]),
            Some(r"C:\Games\Celeste\x64\Celeste.exe".to_owned())
        );

        // Gameplay binary over launcher
        let launcher_vs_game = vec![
            r"C:\Games\The Witcher 3\Launcher.exe".to_owned(),
            r"C:\Games\The Witcher 3\bin\x64\witcher3.exe".to_owned(),
        ];
        assert_eq!(
            preferred_target(r"C:\Games\The Witcher 3", &launcher_vs_game, &[]),
            Some(r"C:\Games\The Witcher 3\bin\x64\witcher3.exe".to_owned())
        );

        // Shallowest path depth tie-breaker
        let nested_vs_root = vec![
            r"C:\Games\Spelunky\tools\Helper.exe".to_owned(),
            r"C:\Games\Spelunky\Spelunky.exe".to_owned(),
        ];
        assert_eq!(
            preferred_target(r"C:\Games\Spelunky", &nested_vs_root, &[]),
            Some(r"C:\Games\Spelunky\Spelunky.exe".to_owned())
        );

        // Manifest launch hint overrides depth and name
        let hinted = vec![
            r"C:\Games\Custom\Game.exe".to_owned(),
            r"C:\Games\Custom\bin\StartCustom.exe".to_owned(),
        ];
        let hints = vec!["<base>/bin/StartCustom.exe".to_owned()];
        assert_eq!(
            preferred_target(r"C:\Games\Custom", &hinted, &hints),
            Some(r"C:\Games\Custom\bin\StartCustom.exe".to_owned())
        );
    }
}
