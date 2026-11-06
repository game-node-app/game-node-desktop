//! How a library entry maps to a GameNode game.
//!
//! Precedence, highest first: the account's own choice (a game, or "no GameNode game"), then a
//! single automatic match from the server, else unmatched. An id the server maps to several games
//! stays unmatched, with those games kept as candidates for the match dialog.

use serde::{Deserialize, Serialize};

use super::store::{AutoMatch, ChosenMatch};

/// Where a matched game came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchSource {
    /// The user picked it.
    Chosen,
    /// The server mapped the store id to exactly one game.
    Automatic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MatchStatus {
    Matched {
        game_id: u64,
        source: MatchSource,
    },
    /// The user marked the entry as having no GameNode game. Sticky: later lookups never change it.
    NoGame,
    /// The server knows several games for this id; the user has not picked one.
    Ambiguous {
        candidates: Vec<u64>,
    },
    Unmatched,
}

impl MatchStatus {
    /// The matched game, if any.
    pub fn game_id(&self) -> Option<u64> {
        match self {
            Self::Matched { game_id, .. } => Some(*game_id),
            _ => None,
        }
    }

    /// Position when sorting by match status: matched first, then the ones needing attention.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Matched { .. } => 0,
            Self::Ambiguous { .. } => 1,
            Self::Unmatched => 2,
            Self::NoGame => 3,
        }
    }
}

/// Resolves one entry from the account's choice and the server's answer for its id.
pub fn resolve(chosen: Option<&ChosenMatch>, auto: Option<&AutoMatch>) -> MatchStatus {
    if let Some(chosen) = chosen {
        return match chosen.game_id {
            Some(game_id) => MatchStatus::Matched {
                game_id,
                source: MatchSource::Chosen,
            },
            None => MatchStatus::NoGame,
        };
    }
    match auto.map(|auto| auto.game_ids.as_slice()) {
        Some([game_id]) => MatchStatus::Matched {
            game_id: *game_id,
            source: MatchSource::Automatic,
        },
        Some(ids) if ids.len() > 1 => MatchStatus::Ambiguous {
            candidates: ids.to_vec(),
        },
        _ => MatchStatus::Unmatched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(ids: &[u64]) -> AutoMatch {
        AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: ids.to_vec(),
            checked_at: 0,
        }
    }

    fn chosen(game_id: Option<u64>) -> ChosenMatch {
        ChosenMatch {
            entry_key: "steam:620".to_owned(),
            game_id,
            chosen_at: 0,
        }
    }

    #[test]
    fn a_single_automatic_match_is_used() {
        assert_eq!(
            resolve(None, Some(&auto(&[7]))),
            MatchStatus::Matched {
                game_id: 7,
                source: MatchSource::Automatic
            }
        );
    }

    #[test]
    fn several_or_no_automatic_matches_stay_unmatched() {
        assert_eq!(
            resolve(None, Some(&auto(&[7, 9]))),
            MatchStatus::Ambiguous {
                candidates: vec![7, 9]
            }
        );
        assert_eq!(resolve(None, Some(&auto(&[]))), MatchStatus::Unmatched);
        assert_eq!(resolve(None, None), MatchStatus::Unmatched);
    }

    #[test]
    fn a_chosen_game_beats_any_automatic_result() {
        for server in [auto(&[7]), auto(&[7, 9]), auto(&[])] {
            assert_eq!(
                resolve(Some(&chosen(Some(42))), Some(&server)),
                MatchStatus::Matched {
                    game_id: 42,
                    source: MatchSource::Chosen
                }
            );
        }
    }

    #[test]
    fn no_game_is_sticky_against_automatic_results() {
        assert_eq!(
            resolve(Some(&chosen(None)), Some(&auto(&[7]))),
            MatchStatus::NoGame
        );
        assert_eq!(resolve(Some(&chosen(None)), None), MatchStatus::NoGame);
    }

    #[test]
    fn match_status_ranks_for_sorting() {
        let matched = MatchStatus::Matched {
            game_id: 1,
            source: MatchSource::Chosen,
        };
        let ambiguous = MatchStatus::Ambiguous { candidates: vec![] };
        assert!(
            matched.rank() < ambiguous.rank()
                && ambiguous.rank() < MatchStatus::Unmatched.rank()
                && MatchStatus::Unmatched.rank() < MatchStatus::NoGame.rank()
        );
    }
}
