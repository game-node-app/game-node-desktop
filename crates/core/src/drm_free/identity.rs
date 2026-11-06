//! Conservative identity inference. Similarity alone never authorizes discovery or a match.
use super::{Evidence, EvidenceKind};
use crate::game_api::GameSummary;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    /// The entry's YAML definition, serialized once at parse time. Keeping text instead of a
    /// `serde_yaml::Value` tree avoids an order-of-magnitude memory overhead for a ~18 MB manifest.
    pub definition: String,
}
#[derive(Clone, Debug, Default)]
pub struct ManifestIndex {
    pub games: BTreeMap<String, ManifestIdentity>,
    /// Normalized title or alias -> game titles.
    by_name: HashMap<String, Vec<String>>,
    /// Normalized install directory name -> game titles.
    by_install: HashMap<String, Vec<String>>,
    /// Lower-cased exact (wildcard-free) launch path -> game titles.
    by_launch: HashMap<String, Vec<String>>,
}

struct ParsedEntry {
    identity: ManifestIdentity,
    alias: Option<String>,
}

fn parse_entry(title: &str, definition: &serde_yaml::Value) -> Result<ParsedEntry, String> {
    if title.len() > 4096 || !definition.is_mapping() {
        return Err("invalid manifest entry".into());
    }
    let keys = |name: &str| {
        definition
            .get(name)
            .and_then(|v| v.as_mapping())
            .map(|v| {
                v.keys()
                    .filter_map(|k| k.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let id = |key| {
        definition
            .get(key)
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_u64())
            .map(|n| n.to_string())
    };
    let alias = definition
        .get("alias")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let text = if alias.is_some() || definition.get("alias").is_some() {
        String::new()
    } else {
        let mut text = serde_yaml::to_string(definition).map_err(|e| e.to_string())?;
        text.shrink_to_fit();
        text
    };
    Ok(ParsedEntry {
        identity: ManifestIdentity {
            title: title.into(),
            aliases: BTreeSet::new(),
            installs: keys("installDir"),
            launch_hints: keys("launch"),
            steam: id("steam"),
            gog: id("gog"),
            definition: text,
        },
        alias,
    })
}

/// Splits a block-style top-level mapping into one text chunk per entry. Top-level keys are the
/// only lines starting in column zero (besides comments and document markers).
fn top_level_chunks(text: &str) -> Vec<&str> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line
            .bytes()
            .next()
            .is_some_and(|b| !matches!(b, b' ' | b'\t' | b'#' | b'\r' | b'\n' | b'-' | b'.'))
        {
            starts.push(offset);
        }
        offset += line.len();
    }
    let mut chunks = Vec::with_capacity(starts.len());
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(text.len());
        chunks.push(&text[start..end]);
    }
    chunks
}

impl ManifestIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("manifest exceeds size limit".into());
        }
        // Parsing entry by entry keeps the peak close to the file size; a single
        // `serde_yaml::Value` for the whole document costs several hundred MB.
        let entries = Self::parse_chunked(bytes).or_else(|_| Self::parse_whole(bytes))?;
        Self::build(entries)
    }

    fn parse_chunked(bytes: &[u8]) -> Result<Vec<ParsedEntry>, String> {
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let chunks = top_level_chunks(text);
        if chunks.is_empty() || chunks.len() > 100_000 {
            return Err("not a chunkable manifest".into());
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let value: serde_yaml::Value =
                serde_yaml::from_str(chunk).map_err(|e| e.to_string())?;
            let mapping = value.as_mapping().ok_or("manifest must be a mapping")?;
            if mapping.len() != 1 {
                return Err("unexpected chunk".into());
            }
            let (title, definition) = mapping.iter().next().unwrap();
            let title = title.as_str().ok_or("invalid title")?;
            if !seen.insert(title.to_owned()) {
                return Err("duplicate manifest entry".into());
            }
            entries.push(parse_entry(title, definition)?);
        }
        Ok(entries)
    }

    fn parse_whole(bytes: &[u8]) -> Result<Vec<ParsedEntry>, String> {
        let value: serde_yaml::Value = serde_yaml::from_slice(bytes).map_err(|e| e.to_string())?;
        let root = value.as_mapping().ok_or("manifest must be a mapping")?;
        if root.len() > 100_000 {
            return Err("too many manifest entries".into());
        }
        root.iter()
            .map(|(title, definition)| {
                parse_entry(title.as_str().ok_or("invalid title")?, definition)
            })
            .collect()
    }

    fn build(entries: Vec<ParsedEntry>) -> Result<Self, String> {
        let mut index = Self::default();
        let mut alias_of: BTreeMap<String, String> = BTreeMap::new();
        for entry in entries {
            match entry.alias {
                Some(target) => {
                    alias_of.insert(entry.identity.title, target);
                }
                None => {
                    index
                        .games
                        .insert(entry.identity.title.clone(), entry.identity);
                }
            }
        }
        for (title, target) in &alias_of {
            let mut target = target.as_str();
            let mut seen = BTreeSet::new();
            for _ in 0..32 {
                if !seen.insert(target) {
                    return Err("manifest alias cycle".into());
                }
                if let Some(next) = alias_of.get(target) {
                    target = next;
                    continue;
                }
                let game = index.games.get_mut(target).ok_or("unknown alias target")?;
                if game.aliases.len() >= 1024 {
                    return Err("too many aliases".into());
                }
                game.aliases.insert(title.clone());
                break;
            }
            if seen.len() >= 32 {
                return Err("manifest alias chain exceeds limit".into());
            }
        }
        index.rebuild_lookups();
        Ok(index)
    }

    fn rebuild_lookups(&mut self) {
        fn add(map: &mut HashMap<String, Vec<String>>, key: String, title: &str) {
            let titles = map.entry(key).or_default();
            if !titles.iter().any(|t| t == title) {
                titles.push(title.to_owned());
            }
        }
        let (mut by_name, mut by_install, mut by_launch) =
            (HashMap::new(), HashMap::new(), HashMap::new());
        for game in self.games.values() {
            for name in std::iter::once(&game.title).chain(game.aliases.iter()) {
                add(&mut by_name, normalized(name), &game.title);
            }
            for install in &game.installs {
                add(&mut by_install, normalized(install), &game.title);
            }
            for hint in &game.launch_hints {
                let hint = hint.replace('\\', "/");
                let hint = hint.strip_prefix("<base>/").unwrap_or(&hint);
                if !hint.contains(['*', '?', '<', '>']) {
                    add(&mut by_launch, hint.to_ascii_lowercase(), &game.title);
                }
            }
        }
        (self.by_name, self.by_install, self.by_launch) = (by_name, by_install, by_launch);
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
        let mut matches: BTreeSet<&String> = BTreeSet::new();
        matches.extend(
            self.by_launch
                .get(&executable.to_ascii_lowercase())
                .into_iter()
                .flatten(),
        );
        for e in evidence {
            if matches!(e.kind, EvidenceKind::Product | EvidenceKind::Description)
                && meaningful(&e.value)
            {
                matches.extend(
                    self.by_name
                        .get(&normalized(&e.value))
                        .into_iter()
                        .flatten(),
                );
            }
        }
        if meaningful(stem)
            && let (Some(by_stem), Some(by_folder)) = (
                self.by_name.get(&normalized(stem)),
                self.by_install.get(&normalized(folder)),
            )
        {
            matches.extend(by_stem.iter().filter(|t| by_folder.contains(t)));
        }
        if matches.len() == 1 {
            let title = matches.first().unwrap();
            result.push(Evidence {
                kind: EvidenceKind::Manifest,
                value: (*title).clone(),
                strong: true,
            });
        }
        result
    }

    pub fn exact(&self, title: &str) -> Vec<&ManifestIdentity> {
        let Some(titles) = self.by_name.get(&normalized(title)) else {
            return Vec::new();
        };
        let ordered: BTreeSet<&String> = titles.iter().collect();
        ordered
            .into_iter()
            .filter_map(|t| self.games.get(t))
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
        definition: game.definition.clone(),
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
    fn chunked_and_whole_document_parsing_agree() {
        let manifest = b"# comment\nCeleste:\n  installDir: {Renamed: {}}\n  launch:\n    Celeste.exe: {}\n  steam: {id: 504230}\n\"Quoted: Title\":\n  files:\n    <base>/save: {tags: [save]}\nOld Name:\n  alias: Celeste\n";
        let chunked =
            ManifestIndex::build(ManifestIndex::parse_chunked(manifest).unwrap()).unwrap();
        let whole = ManifestIndex::build(ManifestIndex::parse_whole(manifest).unwrap()).unwrap();
        assert_eq!(chunked.games.len(), 2);
        for (title, game) in &chunked.games {
            let other = &whole.games[title];
            assert_eq!(game.definition, other.definition);
            assert_eq!(game.aliases, other.aliases);
            assert_eq!(game.installs, other.installs);
            assert_eq!(game.launch_hints, other.launch_hints);
            assert_eq!(game.steam, other.steam);
        }
        assert!(
            chunked.games["Quoted: Title"]
                .definition
                .contains("<base>/save")
        );
        assert_eq!(chunked.exact("old name")[0].title, "Celeste");
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
