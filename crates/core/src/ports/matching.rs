//! Matching a completed port install to a GameNode game (design D11).
//!
//! A port installed through GameNode becomes an ordinary DRM-free registration, so the library's
//! own local matcher is what decides which GameNode game it is. This module is the glue the
//! install engine hands a completion to:
//!
//! - when the overlay names a GameNode game id, that id is used as an external identity, the
//!   strongest evidence the matcher accepts. No title search is involved;
//! - otherwise the upstream game title is offered as title evidence and the conservative rules of
//!   [`crate::drm_free::identity`] decide whether it is unique. The caller supplies the search
//!   results, because only the app can search GameNode;
//! - either way, a choice the user already made (a game, or "no GameNode game") wins. An install
//!   never rewrites what the user chose.
//!
//! Everything here is pure: no I/O, no network and no clock. The app applies the returned status to
//! its library state and persists the accompanying evidence with the local inference.

use serde::{Deserialize, Serialize};

use crate::drm_free::{Evidence, EvidenceKind, identity};
use crate::game_api::GameSummary;
use crate::library::matching::{self, MatchSource, MatchStatus};
use crate::library::store::ChosenMatch;
use crate::ports::catalog::CatalogEntry;

/// What a completed port install offers the local matcher (design D11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PortMatchInput {
    /// The overlay names the GameNode game id.
    GameNodeId { game_id: u64 },
    /// The upstream's title for the original game, as title evidence.
    Title { title: String },
    /// Neither the overlay nor the upstream names anything usable.
    None,
}

impl PortMatchInput {
    /// Reads the input from a merged catalog entry.
    ///
    /// The overlay id wins over every title. Without one, the first original game title is used,
    /// falling back to the entry name when the upstream lists no games.
    pub fn from_entry(entry: &CatalogEntry) -> Self {
        if let Some(game_id) = entry.gamenode_game_id {
            return Self::GameNodeId { game_id };
        }
        if let Some(title) = entry
            .game_titles()
            .map(str::trim)
            .find(|title| !title.is_empty())
        {
            return Self::Title {
                title: title.to_string(),
            };
        }
        match entry.name.trim() {
            "" => Self::None,
            name => Self::Title {
                title: name.to_string(),
            },
        }
    }

    /// The evidence stored with the local inference.
    ///
    /// An overlay id is an [`EvidenceKind::ExternalId`], which ranks like a uniquely mapped store id
    /// and cannot be mistaken for a title. A title is [`EvidenceKind::Manifest`] evidence: a trusted
    /// catalog title, still subject to the conservative uniqueness rules.
    pub fn evidence(&self) -> Vec<Evidence> {
        match self {
            Self::GameNodeId { game_id } => vec![Evidence {
                kind: EvidenceKind::ExternalId,
                value: game_id.to_string(),
                strong: true,
            }],
            Self::Title { title } => vec![Evidence {
                kind: EvidenceKind::Manifest,
                value: title.clone(),
                strong: true,
            }],
            Self::None => Vec::new(),
        }
    }
}

/// The search results a title needs. Only the app can search GameNode, so it passes the answer in.
#[derive(Clone, Copy, Debug)]
pub struct PortMatchSearch<'a> {
    /// The games the server returned for the title, in any order.
    pub games: &'a [GameSummary],
    /// True only when the server said the search was complete (not truncated).
    pub complete: bool,
}

/// What a completed install decided about the entry's GameNode game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortMatchDecision {
    /// The library status the install proposes.
    pub status: MatchStatus,
    /// The evidence the decision was made from, stored with the local inference.
    pub evidence: Vec<Evidence>,
}

impl PortMatchDecision {
    /// The matched GameNode game, if the install proposes one.
    pub fn game_id(&self) -> Option<u64> {
        self.status.game_id()
    }

    /// True when the matcher chose the game itself; false when the user's choice stood or nothing
    /// matched.
    pub fn is_automatic(&self) -> bool {
        matches!(
            self.status,
            MatchStatus::Matched {
                source: MatchSource::Automatic,
                ..
            }
        )
    }
}

/// Resolves a finished install's evidence against the user's choice and, for a title, the search.
///
/// `chosen` is the user's existing choice for the entry, when there is one. It always wins,
/// including "no GameNode game". Without a choice, an overlay id matches immediately; a title goes
/// through [`identity::infer`] and [`identity::unique_exact`], which require a strong identity, an
/// exact normalized title and a complete, unambiguous search.
pub fn decide(
    input: &PortMatchInput,
    chosen: Option<&ChosenMatch>,
    search: Option<PortMatchSearch<'_>>,
) -> PortMatchDecision {
    let evidence = input.evidence();
    if let Some(chosen) = chosen {
        // `resolve` gives the choice the same precedence the rest of the library gives it.
        return PortMatchDecision {
            status: matching::resolve(Some(chosen), None),
            evidence,
        };
    }

    let status = match input {
        PortMatchInput::GameNodeId { game_id } => MatchStatus::Matched {
            game_id: *game_id,
            source: MatchSource::Automatic,
        },
        PortMatchInput::Title { .. } => match search {
            Some(search) => match identity::infer(&evidence) {
                Some(identity) => {
                    match identity::unique_exact(&identity, search.games, search.complete) {
                        Some(game_id) => MatchStatus::Matched {
                            game_id,
                            source: MatchSource::Automatic,
                        },
                        // Ambiguous, weakly evidenced or truncated searches stay unmatched: the entry is
                        // still offered as a suggestion in the match dialog.
                        None => MatchStatus::Unmatched,
                    }
                }
                None => MatchStatus::Unmatched,
            },
            None => MatchStatus::Unmatched,
        },
        PortMatchInput::None => MatchStatus::Unmatched,
    };
    PortMatchDecision { status, evidence }
}

/// Builds the durable local inference for a completed port install (design D11).
///
/// The app stores it with [`crate::drm_free::LocalGameStore::save_inference`] and applies the same
/// decision to its library state. `account` is the signed-in GameNode account id; an install never
/// invents one.
pub fn inference(
    input: &PortMatchInput,
    registration: &crate::drm_free::Registration,
    account: &str,
    decision: &PortMatchDecision,
) -> crate::drm_free::Inference {
    crate::drm_free::Inference {
        installation: registration.id.clone(),
        revision: registration.revision,
        title: match input {
            PortMatchInput::Title { title } => title.clone(),
            PortMatchInput::GameNodeId { .. } | PortMatchInput::None => registration.title.clone(),
        },
        evidence: decision.evidence.clone(),
        game_id: decision.game_id(),
        account: account.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::catalog::CatalogGame;

    fn entry(game_id: Option<u64>, titles: &[&str]) -> CatalogEntry {
        CatalogEntry {
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-bloodborne-pc-windows-port".into(),
            name: "Bloodborne".into(),
            games: titles
                .iter()
                .map(|title| CatalogGame {
                    title: (*title).into(),
                    ..CatalogGame::default()
                })
                .collect(),
            gamenode_game_id: game_id,
            ..CatalogEntry::default()
        }
    }

    fn chosen(game_id: Option<u64>) -> ChosenMatch {
        ChosenMatch {
            entry_key: "manual:install-1".into(),
            game_id,
            chosen_at: 10,
        }
    }

    fn games(names: &[&str]) -> Vec<GameSummary> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| GameSummary {
                id: index as u64 + 1,
                name: (*name).into(),
                release_year: None,
                cover_url: None,
            })
            .collect()
    }

    #[test]
    fn an_overlay_game_id_matches_without_a_search() {
        let input = PortMatchInput::from_entry(&entry(Some(42), &["Bloodborne"]));
        assert_eq!(input, PortMatchInput::GameNodeId { game_id: 42 });
        let decision = decide(&input, None, None);
        assert_eq!(
            decision.status,
            MatchStatus::Matched {
                game_id: 42,
                source: MatchSource::Automatic
            }
        );
        assert!(decision.is_automatic());
        assert_eq!(decision.game_id(), Some(42));
        assert_eq!(
            decision.evidence,
            vec![Evidence {
                kind: EvidenceKind::ExternalId,
                value: "42".into(),
                strong: true
            }]
        );
    }

    #[test]
    fn without_an_overlay_id_the_upstream_title_uses_the_conservative_rules() {
        let input = PortMatchInput::from_entry(&entry(None, &["Celeste"]));
        assert_eq!(
            input,
            PortMatchInput::Title {
                title: "Celeste".into()
            }
        );
        let results = games(&["Celeste"]);
        let decision = decide(
            &input,
            None,
            Some(PortMatchSearch {
                games: &results,
                complete: true,
            }),
        );
        assert_eq!(
            decision.status,
            MatchStatus::Matched {
                game_id: 1,
                source: MatchSource::Automatic
            }
        );

        // Several exact titles, a truncated search or no search at all never pick one.
        let ambiguous = games(&["Celeste", "Celeste"]);
        for search in [
            Some(PortMatchSearch {
                games: &ambiguous,
                complete: true,
            }),
            Some(PortMatchSearch {
                games: &results,
                complete: false,
            }),
            None,
        ] {
            assert_eq!(decide(&input, None, search).status, MatchStatus::Unmatched);
        }

        // A different title is not a fuzzy match.
        let other = games(&["Celeste Classic"]);
        assert_eq!(
            decide(
                &input,
                None,
                Some(PortMatchSearch {
                    games: &other,
                    complete: true
                })
            )
            .status,
            MatchStatus::Unmatched
        );
    }

    #[test]
    fn a_choice_the_user_made_wins_over_the_overlay_id() {
        let input = PortMatchInput::from_entry(&entry(Some(42), &["Bloodborne"]));
        let decision = decide(&input, Some(&chosen(Some(7))), None);
        assert_eq!(
            decision.status,
            MatchStatus::Matched {
                game_id: 7,
                source: MatchSource::Chosen
            }
        );
        assert!(!decision.is_automatic());

        // "No GameNode game" is sticky too: an install never turns it into an automatic match.
        let no_game = decide(&input, Some(&chosen(None)), None);
        assert_eq!(no_game.status, MatchStatus::NoGame);
        assert_eq!(no_game.game_id(), None);
    }

    #[test]
    fn an_entry_without_a_game_id_or_title_matches_nothing() {
        let mut bare = entry(None, &[]);
        bare.name = "   ".into();
        let input = PortMatchInput::from_entry(&bare);
        assert_eq!(input, PortMatchInput::None);
        assert_eq!(decide(&input, None, None).status, MatchStatus::Unmatched);
        assert!(input.evidence().is_empty());
    }

    #[test]
    fn the_entry_name_stands_in_when_the_upstream_lists_no_games() {
        let input = PortMatchInput::from_entry(&entry(None, &[]));
        assert_eq!(
            input,
            PortMatchInput::Title {
                title: "Bloodborne".into()
            }
        );
    }

    #[test]
    fn the_decision_round_trips_for_the_app() {
        let decision = decide(&PortMatchInput::GameNodeId { game_id: 42 }, None, None);
        let json = serde_json::to_string(&decision).expect("serializable");
        assert!(json.contains("\"gameId\":42"), "{json}");
        assert!(json.contains("\"externalId\""), "{json}");
        assert_eq!(
            serde_json::from_str::<PortMatchDecision>(&json).expect("deserializable"),
            decision
        );
    }

    #[test]
    fn the_inference_carries_the_account_and_the_evidence() {
        let registration = crate::drm_free::Registration {
            id: "install-1".into(),
            title: "Bloodborne".into(),
            executable: r"D:\Ports\Bloodborne\Lantern Engine.exe".into(),
            file_identity: "1:2:3".into(),
            root: r"D:\Ports\Bloodborne".into(),
            working_directory: r"D:\Ports\Bloodborne".into(),
            arguments: Vec::new(),
            gameplay: vec!["Bloodborne.exe".into()],
            launchers: vec!["Lantern Engine.exe".into()],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available: true,
        };
        let input = PortMatchInput::GameNodeId { game_id: 42 };
        let decision = decide(&input, None, None);
        let inferred = inference(&input, &registration, "account-1", &decision);
        assert_eq!(inferred.installation, "install-1");
        assert_eq!(inferred.revision, 1);
        assert_eq!(inferred.title, "Bloodborne");
        assert_eq!(inferred.game_id, Some(42));
        assert_eq!(inferred.account, "account-1");
        assert_eq!(inferred.evidence, decision.evidence);
    }
}
