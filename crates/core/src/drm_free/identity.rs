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
pub fn maintenance(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "setup",
        "install",
        "unins",
        "update",
        "crash",
        "redist",
        "vcredist",
        "dxsetup",
        "support",
        "unitycrashhandler",
    ]
    .iter()
    .any(|word| lower.starts_with(word))
}
pub fn meaningful(title: &str) -> bool {
    let normalized = normalized(title);
    !normalized.is_empty()
        && !maintenance(&normalized)
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
    let conflict = usable
        .iter()
        .any(|e| e.strong && self::normalized(&e.value) != normalized);
    Some(TitleIdentity {
        title: first.value.clone(),
        strong: !conflict && (first.strong || corroborated),
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
    fn aliases_are_bounded_and_unknown_fields_tolerated() {
        let index =
            ManifestIndex::parse(b"Game:\n  files: {}\n  future: true\nOld:\n  alias: Game\n")
                .unwrap();
        assert_eq!(index.exact("Old")[0].title, "Game");
        assert!(ManifestIndex::parse(b"A:\n  alias: B\nB:\n  alias: A\n").is_err());
    }
}

pub fn profile(game:&ManifestIdentity,installation:&super::Registration,hash:&str,id:String)->Result<super::SaveProfile,String> {
    Ok(super::SaveProfile { id,installation:installation.id.clone(),revision:1,title:game.title.clone(),manifest_hash:hash.into(),definition:serde_yaml::to_string(&game.definition).map_err(|e|e.to_string())?,files:vec![],registry:vec![],support:super::SaveSupport::Supported,automatic:None })
}
