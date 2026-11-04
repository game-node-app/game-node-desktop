//! Pure serialized submit decisions. A pass stops on transient failure or rejected credentials.
use super::{Submission, SubmitError, Total};
use crate::sync::SyncPreferences;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub total: Total,
    pub game_id: Option<u64>,
    pub source: String,
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
    fn candidate() -> Candidate {
        Candidate {
            total: Total {
                account_id: "a".into(),
                entry_key: "steam:620".into(),
                total_seconds: 100,
                last_played_at: Some(10),
                accepted_game_id: None,
                accepted_seconds: None,
                rejected_seconds: None,
            },
            game_id: Some(620),
            source: "steam".into(),
        }
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
}
