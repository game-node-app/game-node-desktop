//! Anti-cheat detection, folder marker inspection, and bundled blocklist rules.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// Known anti-cheat technologies and blocklist categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntiCheatSystem {
    EasyAntiCheat,
    BattlEye,
    Blocklisted,
}

impl std::fmt::Display for AntiCheatSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EasyAntiCheat => write!(f, "Easy Anti-Cheat"),
            Self::BattlEye => write!(f, "BattlEye"),
            Self::Blocklisted => write!(f, "Blocklisted Competitive Game"),
        }
    }
}

/// Evaluation report indicating whether anti-cheat markers or blocklisted titles were found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntiCheatReport {
    pub detected: Vec<AntiCheatSystem>,
    pub evidence: Vec<String>,
}

impl AntiCheatReport {
    /// Returns true if any anti-cheat system or blocklist match was found.
    pub fn has_warning(&self) -> bool {
        !self.detected.is_empty()
    }
}

/// Bundled blocklist rules for competitive titles that ship kernel/client anti-cheat or ban
/// modded DLLs.
///
/// The rules are keyed by the full provider entry key (`steam:<appId>`, `epic:<appName>`, ...), so
/// adding a store never needs a host code change.
const BLOCKLISTED_ENTRIES_TOML: &str = include_str!("../../data/anticheat.toml");

#[derive(Debug, Deserialize)]
struct BlocklistFile {
    #[serde(default)]
    blocked: Vec<BlocklistEntry>,
}

#[derive(Debug, Deserialize)]
struct BlocklistEntry {
    entry: String,
    title: String,
}

/// Parses the bundled blocklist once and indexes it by full entry key.
fn blocked_entries() -> &'static HashMap<String, String> {
    static ENTRIES: OnceLock<HashMap<String, String>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let file: BlocklistFile = toml::from_str(BLOCKLISTED_ENTRIES_TOML)
            .expect("bundled anti-cheat blocklist must be valid TOML");
        file.blocked
            .into_iter()
            .map(|blocked| (blocked.entry, blocked.title))
            .collect()
    })
}

/// Known game titles (or keywords) that feature strict anti-cheat.
const BLOCKLISTED_TITLES: &[&str] = &[
    "apex legends",
    "destiny 2",
    "pubg",
    "rainbow six siege",
    "tom clancy's rainbow six siege",
    "rust",
    "call of duty",
    "halo infinite",
    "battlefield 2042",
    "helldivers 2",
    "counter-strike 2",
    "fortnite",
    "valorant",
];

/// Pure anti-cheat evaluation combining folder markers and blocklisted entry keys/titles.
pub fn evaluate_anticheat<'a>(
    entry_key: &str,
    title: &str,
    relative_file_paths: impl IntoIterator<Item = &'a str>,
) -> AntiCheatReport {
    let mut detected_systems = BTreeSet::new();
    let mut evidence = Vec::new();

    // 1. Bundled blocklist check by full entry key (e.g. "steam:1172470").
    if let Some(blocked_title) = blocked_entries().get(entry_key) {
        detected_systems.insert(AntiCheatSystem::Blocklisted);
        evidence.push(format!(
            "Entry {entry_key} matches known competitive title: {blocked_title}"
        ));
    }

    // 2. Title check
    let normalized_title = title.trim().to_lowercase();
    if !normalized_title.is_empty() {
        for blocked in BLOCKLISTED_TITLES {
            if normalized_title == *blocked || normalized_title.contains(blocked) {
                detected_systems.insert(AntiCheatSystem::Blocklisted);
                evidence.push(format!(
                    "Title '{title}' matches blocklist entry '{blocked}'"
                ));
                break;
            }
        }
    }

    // 3. Folder markers and binaries check
    for path in relative_file_paths {
        let normalized_path = path.replace('\\', "/");
        let lower = normalized_path.to_lowercase();

        // Check path components for folder names
        let parts: Vec<&str> = lower.split('/').collect();
        let filename = parts.last().copied().unwrap_or("");

        // EasyAntiCheat detection
        let is_eac_dir = parts
            .iter()
            .take(parts.len().saturating_sub(1))
            .any(|&p| p == "easyanticheat");
        let is_eac_file =
            filename.starts_with("easyanticheat") || filename.starts_with("install_easyanticheat");

        if is_eac_dir || is_eac_file {
            detected_systems.insert(AntiCheatSystem::EasyAntiCheat);
            evidence.push(format!("Found EasyAntiCheat marker: {path}"));
        }

        // BattlEye detection
        let is_be_dir = parts
            .iter()
            .take(parts.len().saturating_sub(1))
            .any(|&p| p == "battleye");
        let is_be_file = filename.starts_with("beservice")
            || filename.starts_with("beclient")
            || filename.starts_with("install_battleye");

        if is_be_dir || is_be_file {
            detected_systems.insert(AntiCheatSystem::BattlEye);
            evidence.push(format!("Found BattlEye marker: {path}"));
        }
    }

    AntiCheatReport {
        detected: detected_systems.into_iter().collect(),
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_single_player_game() {
        let report = evaluate_anticheat(
            "steam:1091500",
            "Cyberpunk 2077",
            ["bin/x64/Cyberpunk2077.exe", "r6/config/settings.json"],
        );
        assert!(!report.has_warning());
        assert!(report.detected.is_empty());
        assert!(report.evidence.is_empty());
    }

    #[test]
    fn test_steam_blocklist_app_id() {
        let report = evaluate_anticheat("steam:1172470", "Apex Legends", []);
        assert!(report.has_warning());
        assert_eq!(report.detected, vec![AntiCheatSystem::Blocklisted]);
        assert!(!report.evidence.is_empty());
    }

    #[test]
    fn test_epic_blocklist_entry_key() {
        // The title deliberately does not match the title heuristics, so only the bundled
        // `epic:` entry key can produce this warning.
        let report = evaluate_anticheat("epic:Fortnite", "Launcher Item", []);
        assert!(report.has_warning());
        assert_eq!(report.detected, vec![AntiCheatSystem::Blocklisted]);
        assert!(
            report
                .evidence
                .iter()
                .any(|evidence| evidence.contains("epic:Fortnite"))
        );
    }

    #[test]
    fn test_bundled_blocklist_is_keyed_by_full_entry_key() {
        let entries = blocked_entries();
        assert_eq!(
            entries.get("steam:1172470").map(String::as_str),
            Some("Apex Legends")
        );
        assert_eq!(
            entries.get("epic:Fortnite").map(String::as_str),
            Some("Fortnite")
        );
        assert!(!entries.contains_key("1172470"));
    }

    #[test]
    fn test_title_blocklist() {
        let report = evaluate_anticheat("manual:fortnite", "Fortnite", []);
        assert!(report.has_warning());
        assert_eq!(report.detected, vec![AntiCheatSystem::Blocklisted]);
    }

    #[test]
    fn test_easyanticheat_folder_and_file() {
        let report = evaluate_anticheat(
            "steam:12345",
            "Some Game",
            ["EasyAntiCheat/EasyAntiCheat_EOS.exe", "Game.exe"],
        );
        assert!(report.has_warning());
        assert_eq!(report.detected, vec![AntiCheatSystem::EasyAntiCheat]);
        assert_eq!(report.evidence.len(), 1);
    }

    #[test]
    fn test_battleye_files() {
        let report = evaluate_anticheat(
            "steam:54321",
            "Tactical Shooter",
            [
                "BattlEye/BEService_x64.exe",
                "BattlEye/BEClient_x64.dll",
                "Binaries/Game.exe",
            ],
        );
        assert!(report.has_warning());
        assert_eq!(report.detected, vec![AntiCheatSystem::BattlEye]);
        assert_eq!(report.evidence.len(), 2);
    }

    #[test]
    fn test_multiple_detections() {
        let report = evaluate_anticheat(
            "steam:252490",
            "Rust",
            ["EasyAntiCheat/EasyAntiCheat.dll", "RustClient.exe"],
        );
        assert!(report.has_warning());
        assert_eq!(
            report.detected,
            vec![AntiCheatSystem::EasyAntiCheat, AntiCheatSystem::Blocklisted]
        );
    }
}
