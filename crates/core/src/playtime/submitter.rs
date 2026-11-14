//! Pure serialized submit decisions. A pass stops on transient failure or rejected credentials.
use super::{Submission, SubmitError, Total};
use crate::sync::SyncPreferences;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The provider that reported the entry; groups never span providers.
    pub provider: String,
    pub total: Total,
    pub game_id: Option<u64>,
    pub source: String,
}

/// The key that stores the accepted/rejected state of a group of entries matched to one game.
pub fn group_key(provider: &str, game_id: u64) -> String {
    format!("{provider}:game:{game_id}")
}

/// Groups per-entry store totals that were matched to the same GameNode game.
///
/// One candidate is emitted per `(account, provider, game)` with the summed seconds and the latest
/// known play date. A group of one keeps that entry's key and stored state, so rows written before
/// grouping existed do not re-submit. Once a synthetic row exists for the game, the group stays
/// tracked under [`group_key`] even when it shrinks to one entry: its next submission is a single
/// replacement total that converges the server to the remaining entry. A group of two or more
/// keeps its state under [`group_key`] in `stored`.
pub fn group_candidates(candidates: Vec<Candidate>, stored: &[Total]) -> Vec<Candidate> {
    struct Group {
        candidate: Candidate,
        members: usize,
    }

    let mut groups: Vec<Group> = Vec::new();
    let mut positions: BTreeMap<(String, String, u64), usize> = BTreeMap::new();
    for candidate in candidates {
        let Some(game_id) = candidate.game_id else {
            // An unmatched entry cannot be grouped and is filtered by the submitter.
            groups.push(Group {
                candidate,
                members: 1,
            });
            continue;
        };
        let key = (
            candidate.total.account_id.clone(),
            candidate.provider.clone(),
            game_id,
        );
        match positions.get(&key) {
            Some(&position) => {
                let group = &mut groups[position];
                group.members += 1;
                group.candidate.total.total_seconds = group
                    .candidate
                    .total
                    .total_seconds
                    .saturating_add(candidate.total.total_seconds);
                group.candidate.total.last_played_at = group
                    .candidate
                    .total
                    .last_played_at
                    .max(candidate.total.last_played_at);
            }
            None => {
                positions.insert(key, groups.len());
                groups.push(Group {
                    candidate,
                    members: 1,
                });
            }
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let Some(game_id) = group.candidate.game_id else {
                // An unmatched entry has no group key to fall back to.
                return group.candidate;
            };
            let key = group_key(&group.candidate.provider, game_id);
            let previous = stored.iter().find(|total| {
                total.account_id == group.candidate.total.account_id && total.entry_key == key
            });
            // A single entry that was never grouped keeps its own key and state. A group with a
            // synthetic row keeps that key even after it shrinks, so the shrinking pass submits
            // one replacement total instead of leaving the server holding the old sum.
            if group.members <= 1 && previous.is_none() {
                return group.candidate;
            }
            Candidate {
                total: Total {
                    account_id: group.candidate.total.account_id,
                    entry_key: key,
                    total_seconds: group.candidate.total.total_seconds,
                    last_played_at: group.candidate.total.last_played_at,
                    accepted_game_id: previous.and_then(|total| total.accepted_game_id),
                    accepted_seconds: previous.and_then(|total| total.accepted_seconds),
                    rejected_seconds: previous.and_then(|total| total.rejected_seconds),
                },
                provider: group.candidate.provider,
                game_id: group.candidate.game_id,
                source: group.candidate.source,
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Send {
        entry_key: String,
        submission: Submission,
    },
    Save(Total),
    SessionRejected,
}
#[derive(Default)]
pub struct Submitter {
    queue: VecDeque<Candidate>,
    in_flight: Option<Candidate>,
}
impl Submitter {
    pub fn start_for_source(
        &mut self,
        source: super::PlaytimeSource,
        account: &str,
        preferences: SyncPreferences,
        verified: bool,
        candidates: Vec<Candidate>,
    ) -> Vec<Effect> {
        if source != super::PlaytimeSource::Store {
            self.cancel();
            return Vec::new();
        }
        self.start(account, preferences, verified, candidates)
    }
    /// Starts only when no previous pass is in progress. Account ownership is checked here too.
    pub fn start(
        &mut self,
        account: &str,
        preferences: SyncPreferences,
        verified: bool,
        candidates: Vec<Candidate>,
    ) -> Vec<Effect> {
        if self.in_flight.is_some() || !preferences.may_submit(verified) {
            return Vec::new();
        }
        self.queue = candidates
            .into_iter()
            .filter(|c| {
                c.total.account_id == account
                    && c.game_id.is_some_and(|game| {
                        (Some(game), Some(c.total.total_seconds))
                            != (c.total.accepted_game_id, c.total.accepted_seconds)
                            && c.total.rejected_seconds != Some(c.total.total_seconds)
                    })
            })
            .collect();
        self.next().into_iter().collect()
    }
    /// Settings/account changes cancel queued work; the engine must discard stale completions.
    pub fn cancel(&mut self) {
        self.queue.clear();
        self.in_flight = None;
    }
    pub fn complete(&mut self, result: Result<(), SubmitError>) -> Vec<Effect> {
        let Some(mut candidate) = self.in_flight.take() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        match result {
            Ok(()) => {
                candidate.total.accepted_game_id = candidate.game_id;
                candidate.total.accepted_seconds = Some(candidate.total.total_seconds);
                effects.push(Effect::Save(candidate.total));
            }
            Err(SubmitError::Rejected) => {
                candidate.total.rejected_seconds = Some(candidate.total.total_seconds);
                effects.push(Effect::Save(candidate.total));
            }
            Err(error) => {
                self.queue.clear();
                if error == SubmitError::Unauthorized {
                    effects.push(Effect::SessionRejected);
                }
                return effects;
            }
        }
        effects.extend(self.next());
        effects
    }
    fn next(&mut self) -> Option<Effect> {
        let candidate = self.queue.pop_front()?;
        let effect = Effect::Send {
            entry_key: candidate.total.entry_key.clone(),
            submission: Submission {
                game_id: candidate.game_id?,
                source: candidate.source.clone(),
                total_seconds: candidate.total.total_seconds,
                last_played_at: candidate.total.last_played_at,
            },
        };
        self.in_flight = Some(candidate);
        Some(effect)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(provider: &str, key: &str, seconds: i64, last_played: Option<i64>) -> Candidate {
        Candidate {
            provider: provider.into(),
            total: Total {
                account_id: "a".into(),
                entry_key: key.into(),
                total_seconds: seconds,
                last_played_at: last_played,
                accepted_game_id: None,
                accepted_seconds: None,
                rejected_seconds: None,
            },
            game_id: Some(7),
            source: "steam".into(),
        }
    }

    fn candidate() -> Candidate {
        entry("steam", "steam:620", 100, Some(10))
    }
    fn on() -> SyncPreferences {
        SyncPreferences {
            enabled: true,
            explained: true,
        }
    }
    #[test]
    fn each_gate_holds_totals_and_reenable_sends() {
        let mut s = Submitter::default();
        for (prefs, verified) in [
            (
                SyncPreferences {
                    enabled: false,
                    ..on()
                },
                true,
            ),
            (SyncPreferences::default(), true),
            (on(), false),
        ] {
            assert!(s.start("a", prefs, verified, vec![candidate()]).is_empty());
        }
        let mut c = candidate();
        c.game_id = None;
        assert!(s.start("a", on(), true, vec![c]).is_empty());
        assert!(s.start("other", on(), true, vec![candidate()]).is_empty());
        assert!(matches!(
            &s.start("a", on(), true, vec![candidate()])[0],
            Effect::Send { .. }
        ));
    }
    #[test]
    fn accepted_totals_wait_for_change_or_rematch() {
        let mut s = Submitter::default();
        s.start("a", on(), true, vec![candidate()]);
        let Effect::Save(total) = s.complete(Ok(())).remove(0) else {
            panic!()
        };
        let mut c = candidate();
        c.total = total;
        assert!(s.start("a", on(), true, vec![c.clone()]).is_empty());
        c.game_id = Some(621);
        assert!(
            matches!(&s.start("a",on(),true,vec![c])[0],Effect::Send{submission,..} if submission.game_id==621)
        );
    }
    #[test]
    fn rejected_value_waits_until_total_changes() {
        let mut s = Submitter::default();
        s.start("a", on(), true, vec![candidate()]);
        let Effect::Save(total) = s.complete(Err(SubmitError::Rejected)).remove(0) else {
            panic!()
        };
        let mut c = candidate();
        c.total = total;
        assert!(s.start("a", on(), true, vec![c.clone()]).is_empty());
        c.total.total_seconds += 1;
        assert!(matches!(
            &s.start("a", on(), true, vec![c])[0],
            Effect::Send { .. }
        ));
    }
    #[test]
    fn serialized_requests_and_failures_stop_the_pass() {
        for result in [
            Ok(()),
            Err(SubmitError::Rejected),
            Err(SubmitError::Unavailable),
            Err(SubmitError::Unauthorized),
        ] {
            let mut s = Submitter::default();
            let mut second = candidate();
            second.total.entry_key = "second".into();
            assert_eq!(s.start("a", on(), true, vec![candidate(), second]).len(), 1);
            assert!(s.start("a", on(), true, vec![candidate()]).is_empty());
            let effects = s.complete(result);
            assert_eq!(
                effects.iter().any(|e| matches!(e, Effect::Send { .. })),
                result.is_ok() || result == Err(SubmitError::Rejected)
            );
            assert_eq!(
                effects.contains(&Effect::SessionRejected),
                result == Err(SubmitError::Unauthorized)
            );
        }
    }

    #[test]
    fn entries_matched_to_one_game_become_one_summed_candidate() {
        let grouped = group_candidates(
            vec![
                entry("steam", "steam:620", 2 * 3600, Some(100)),
                entry("steam", "steam:621", 3 * 3600, Some(200)),
            ],
            &[],
        );
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].total.entry_key, "steam:game:7");
        assert_eq!(grouped[0].total.total_seconds, 5 * 3600);
        assert_eq!(grouped[0].total.last_played_at, Some(200));

        let mut s = Submitter::default();
        let effects = s.start("a", on(), true, grouped);
        assert_eq!(effects.len(), 1);
        let Effect::Send {
            entry_key,
            submission,
        } = &effects[0]
        else {
            panic!("{effects:?}")
        };
        assert_eq!(entry_key, "steam:game:7");
        assert_eq!(submission.total_seconds, 5 * 3600);
        assert_eq!(submission.game_id, 7);
    }

    #[test]
    fn an_unchanged_sum_is_not_requested_again() {
        let candidates = || {
            vec![
                entry("steam", "steam:620", 2 * 3600, Some(100)),
                entry("steam", "steam:621", 3 * 3600, Some(200)),
            ]
        };
        let mut s = Submitter::default();
        s.start("a", on(), true, group_candidates(candidates(), &[]));
        let Effect::Save(accepted) = s.complete(Ok(())).remove(0) else {
            panic!()
        };
        assert_eq!(accepted.entry_key, "steam:game:7");

        // The next read yields the same sum; its state is the group's stored row.
        let grouped = group_candidates(candidates(), &[accepted]);
        assert!(s.start("a", on(), true, grouped).is_empty());
    }

    #[test]
    fn a_group_that_shrinks_submits_one_replacement_total_and_then_settles() {
        let mut s = Submitter::default();
        let mut first = entry("steam", "steam:620", 2 * 3600, Some(100));
        first.total.accepted_game_id = Some(7);
        first.total.accepted_seconds = Some(2 * 3600);

        // One entry with no synthetic row keeps its own key and accepted state, exactly as before
        // grouping existed.
        let grouped = group_candidates(vec![first.clone()], &[]);
        assert_eq!(grouped[0].total.entry_key, "steam:620");
        assert!(s.start("a", on(), true, grouped).is_empty());

        // A second entry joins: the group switches to the synthetic key and sends the new sum.
        let joined = group_candidates(
            vec![
                first.clone(),
                entry("steam", "steam:621", 3 * 3600, Some(200)),
            ],
            &[],
        );
        assert_eq!(joined[0].total.entry_key, "steam:game:7");
        let effects = s.start("a", on(), true, joined);
        assert_eq!(effects.len(), 1);
        let Effect::Save(accepted) = s.complete(Ok(())).remove(0) else {
            panic!()
        };
        assert_eq!(accepted.entry_key, "steam:game:7");
        assert_eq!(accepted.accepted_seconds, Some(5 * 3600));

        // The second entry leaves: the synthetic row still exists, so exactly one replacement total
        // converges the server from 5h to the remaining 2h.
        let left = group_candidates(vec![first.clone()], std::slice::from_ref(&accepted));
        assert_eq!(left[0].total.entry_key, "steam:game:7");
        assert_eq!(left[0].total.accepted_seconds, Some(5 * 3600));
        let effects = s.start("a", on(), true, left);
        assert_eq!(
            effects,
            vec![Effect::Send {
                entry_key: "steam:game:7".into(),
                submission: Submission {
                    game_id: 7,
                    source: "steam".into(),
                    total_seconds: 2 * 3600,
                    last_played_at: Some(100),
                },
            }]
        );
        let Effect::Save(converged) = s.complete(Ok(())).remove(0) else {
            panic!()
        };
        assert_eq!(converged.entry_key, "steam:game:7");
        assert_eq!(converged.accepted_seconds, Some(2 * 3600));

        // The following identical read sends nothing.
        let settled = group_candidates(vec![first.clone()], std::slice::from_ref(&converged));
        assert_eq!(settled[0].total.entry_key, "steam:game:7");
        assert!(s.start("a", on(), true, settled).is_empty());

        // A changed total on the remaining entry is sent once more, still under the group's key.
        let mut changed = first;
        changed.total.total_seconds = 2 * 3600 + 600;
        let changed = group_candidates(vec![changed], std::slice::from_ref(&converged));
        assert_eq!(
            s.start("a", on(), true, changed)[0],
            Effect::Send {
                entry_key: "steam:game:7".into(),
                submission: Submission {
                    game_id: 7,
                    source: "steam".into(),
                    total_seconds: 2 * 3600 + 600,
                    last_played_at: Some(100),
                },
            }
        );
    }

    #[test]
    fn single_entries_and_other_games_or_providers_do_not_group() {
        let mut other_game = entry("steam", "steam:730", 60, None);
        other_game.game_id = Some(8);
        let mut other_provider = entry("epic", "epic:app", 30, None);
        other_provider.game_id = Some(7);
        other_provider.source = "epicgames".into();
        let grouped = group_candidates(
            vec![
                entry("steam", "steam:620", 2 * 3600, Some(100)),
                other_game,
                other_provider,
            ],
            &[],
        );
        assert_eq!(grouped.len(), 3);
        assert_eq!(
            grouped
                .iter()
                .map(|c| c.total.entry_key.as_str())
                .collect::<Vec<_>>(),
            ["steam:620", "steam:730", "epic:app"]
        );
    }

    #[test]
    fn the_latest_known_date_wins_and_absent_dates_stay_unknown() {
        let grouped = group_candidates(
            vec![
                entry("steam", "steam:620", 60, Some(100)),
                entry("steam", "steam:621", 60, None),
            ],
            &[],
        );
        assert_eq!(grouped[0].total.last_played_at, Some(100));

        let unknown = group_candidates(
            vec![
                entry("steam", "steam:620", 60, None),
                entry("steam", "steam:621", 60, None),
            ],
            &[],
        );
        assert_eq!(unknown[0].total.last_played_at, None);
    }

    #[test]
    fn existing_single_entry_rows_do_not_resubmit() {
        let mut total = entry("steam", "steam:620", 45240, Some(100));
        total.total.accepted_game_id = Some(7);
        total.total.accepted_seconds = Some(45240);
        let grouped = group_candidates(vec![total], &[]);
        let mut s = Submitter::default();
        assert!(s.start("a", on(), true, grouped).is_empty());
    }
}
