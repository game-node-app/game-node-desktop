//! Core arbitration for plugin-reported runtime gameplay observations.
//!
//! Runtime adapters report content identity and lifecycle; they never provide process identity
//! or a measured duration. This state machine emits the same durable session effects consumed by
//! the process tracker, bounded by the last fresh observation.

use crate::playtime::Session;
use std::collections::{BTreeMap, VecDeque};

/// Reports older than this are not allowed to extend a measured interval.
pub const FRESHNESS_SECONDS: i64 = 30;
const FUTURE_SKEW_SECONDS: i64 = 5;
const MAX_ID_BYTES: usize = 256;
const MAX_INSTANCES: usize = 256;
const MAX_DEDUPLICATION_IDS: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted { host_session_id: Option<String> },
    Duplicate,
    Invalid(&'static str),
    Stale,
    Conflict(&'static str),
    Unavailable(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Identified,
    Started,
    Paused,
    Resumed,
    ContentChanged,
    Ended,
    IdentificationLost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub correlation_id: String,
    pub game_id: u64,
    pub installation_id: String,
    pub content_id: String,
    pub runtime_id: String,
    pub runtime_instance_id: String,
    pub reporter_session_id: String,
    pub sequence: u64,
    pub observed_at: i64,
    pub transition: Transition,
    pub source_observation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Open(Session),
    Checkpoint {
        id: String,
        at: i64,
    },
    Close {
        id: String,
        ended_at: i64,
        game_id: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct InstanceKey {
    plugin_id: String,
    reporter_session_id: String,
    runtime_instance_id: String,
}

#[derive(Clone, Debug)]
struct Active {
    game_id: u64,
    installation_id: String,
    content_id: String,
    runtime_id: String,
    sequence: u64,
    observed_at: i64,
    session: Option<Session>,
    terminal: bool,
}

/// Pure state machine. The host supplies authenticated plugin identity, account/source context,
/// current time and session ids; all persistence remains with the engine.
#[derive(Default)]
pub struct RuntimeSessions {
    instances: BTreeMap<InstanceKey, Active>,
    observations: BTreeMap<String, Report>,
    observation_order: VecDeque<String>,
}

impl RuntimeSessions {
    pub fn report(
        &mut self,
        plugin_id: &str,
        account_id: &str,
        source: &str,
        report: Report,
        now: i64,
        mut next_id: impl FnMut() -> String,
    ) -> (Outcome, Vec<Effect>) {
        if !valid_id(plugin_id)
            || !valid_id(account_id)
            || !valid_id(source)
            || !valid_id(&report.correlation_id)
            || report.game_id == 0
            || !valid_id(&report.installation_id)
            || !valid_id(&report.content_id)
            || !valid_id(&report.runtime_id)
            || !valid_id(&report.runtime_instance_id)
            || !valid_id(&report.reporter_session_id)
            || !valid_id(&report.source_observation_id)
            || report.sequence == 0
        {
            return (Outcome::Invalid("missing or malformed identity"), vec![]);
        }
        if report.observed_at < now.saturating_sub(FRESHNESS_SECONDS)
            || report.observed_at > now.saturating_add(FUTURE_SKEW_SECONDS)
        {
            return (Outcome::Stale, vec![]);
        }
        let transition = report.transition;
        let game_id = report.game_id;

        if let Some(previous) = self.observations.get(&report.source_observation_id) {
            return if previous.game_id == report.game_id
                && previous.installation_id == report.installation_id
                && previous.content_id == report.content_id
                && previous.runtime_id == report.runtime_id
                && previous.observed_at == report.observed_at
                && previous.transition == report.transition
            {
                (Outcome::Duplicate, vec![])
            } else {
                (Outcome::Conflict("observation id was reused"), vec![])
            };
        }
        let key = InstanceKey {
            plugin_id: plugin_id.to_owned(),
            reporter_session_id: report.reporter_session_id.clone(),
            runtime_instance_id: report.runtime_instance_id.clone(),
        };
        if !self.instances.contains_key(&key) && self.instances.len() >= MAX_INSTANCES {
            return (
                Outcome::Unavailable("runtime instance capacity reached"),
                vec![],
            );
        }
        if self.instances.get(&key).is_some_and(|active| {
            active.terminal
                || report.sequence <= active.sequence
                || report.observed_at < active.observed_at
                || report.game_id != active.game_id
                || report.installation_id != active.installation_id
                || report.runtime_id != active.runtime_id
        }) {
            return (
                Outcome::Conflict("sequence or runtime identity changed"),
                vec![],
            );
        }

        let mut effects = Vec::new();
        let mut opened = None;
        let at = report.observed_at.min(now);
        let existing = self.instances.remove(&key);
        let mut current = existing.unwrap_or_else(|| Active {
            game_id,
            installation_id: report.installation_id.clone(),
            content_id: report.content_id.clone(),
            runtime_id: report.runtime_id.clone(),
            sequence: 0,
            observed_at: at,
            session: None,
            terminal: false,
        });
        let content_changed = current.content_id != report.content_id;
        let was_active = current.session.is_some();
        if content_changed {
            close(&mut current, at, &mut effects);
            current.content_id.clone_from(&report.content_id);
        }

        match transition {
            Transition::Identified => {}
            Transition::Started | Transition::Resumed => {
                if current.session.is_none() {
                    let session = Session {
                        id: next_id(),
                        account_id: account_id.to_owned(),
                        entry_key: report.installation_id.clone(),
                        game_id: Some(game_id),
                        source: source.to_owned(),
                        platform_id: 6,
                        started_at: at,
                        checkpoint_at: at,
                        ended_at: None,
                    };
                    opened = Some(session.id.clone());
                    effects.push(Effect::Open(session.clone()));
                    current.session = Some(session);
                }
            }
            Transition::Paused | Transition::Ended | Transition::IdentificationLost => {
                close(&mut current, at, &mut effects)
            }
            Transition::ContentChanged => {
                if was_active && !content_changed {
                    return (
                        Outcome::Conflict("content change omitted a new content identity"),
                        vec![],
                    );
                }
                if was_active {
                    let session = Session {
                        id: next_id(),
                        account_id: account_id.to_owned(),
                        entry_key: report.installation_id.clone(),
                        game_id: Some(game_id),
                        source: source.to_owned(),
                        platform_id: 6,
                        started_at: at,
                        checkpoint_at: at,
                        ended_at: None,
                    };
                    opened = Some(session.id.clone());
                    effects.push(Effect::Open(session.clone()));
                    current.session = Some(session);
                }
            }
        }

        current.sequence = report.sequence;
        current.observed_at = at;
        current.terminal = transition == Transition::Ended;
        let observation_id = report.source_observation_id.clone();
        self.observations.insert(observation_id.clone(), report);
        self.observation_order.push_back(observation_id);
        while self.observation_order.len() > MAX_DEDUPLICATION_IDS {
            if let Some(expired) = self.observation_order.pop_front() {
                self.observations.remove(&expired);
            }
        }
        self.instances.insert(key, current);
        (
            Outcome::Accepted {
                host_session_id: opened,
            },
            effects,
        )
    }

    /// Closes at the last supported observation, never at the later time of discovery.
    pub fn expire(&mut self, now: i64) -> Vec<Effect> {
        let expired: Vec<_> = self
            .instances
            .iter()
            .filter(|(_, active)| now.saturating_sub(active.observed_at) > FRESHNESS_SECONDS)
            .map(|(key, _)| key.clone())
            .collect();
        let mut effects = Vec::new();
        for key in expired {
            if let Some(active) = self.instances.get_mut(&key) {
                close(active, active.observed_at, &mut effects);
            }
        }
        effects
    }

    /// Disconnect/revocation fences all intervals for a reporter at its last accepted evidence.
    pub fn fence_plugin(&mut self, plugin_id: &str) -> Vec<Effect> {
        let keys: Vec<_> = self
            .instances
            .keys()
            .filter(|key| key.plugin_id == plugin_id)
            .cloned()
            .collect();
        let mut effects = Vec::new();
        for key in keys {
            if let Some(mut active) = self.instances.remove(&key) {
                let last_observed_at = active.observed_at;
                close(&mut active, last_observed_at, &mut effects);
            }
        }
        effects
    }

    pub fn fence_entry(&mut self, entry_key: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        for active in self.instances.values_mut() {
            if active
                .session
                .as_ref()
                .is_some_and(|session| session.entry_key == entry_key)
            {
                close(active, active.observed_at, &mut effects);
            }
        }
        effects
    }

    pub fn reset(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for active in self.instances.values_mut() {
            close(active, active.observed_at, &mut effects);
        }
        self.instances.clear();
        self.observations.clear();
        self.observation_order.clear();
        effects
    }
}

fn close(active: &mut Active, at: i64, effects: &mut Vec<Effect>) {
    if let Some(session) = active.session.take() {
        let ended_at = at.max(session.started_at);
        effects.push(Effect::Close {
            id: session.id,
            ended_at,
            game_id: Some(active.game_id),
        });
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && !value
            .chars()
            .any(|character| character.is_control() || character == '\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(sequence: u64, transition: Transition, at: i64, content_id: &str) -> Report {
        Report {
            correlation_id: format!("corr-{sequence}"),
            game_id: 42,
            installation_id: "install-a".into(),
            content_id: content_id.into(),
            runtime_id: "emulator-a".into(),
            runtime_instance_id: "instance-a".into(),
            reporter_session_id: "reporter-a".into(),
            sequence,
            observed_at: at,
            transition,
            source_observation_id: format!("observation-{sequence}"),
        }
    }

    fn apply(
        tracker: &mut RuntimeSessions,
        value: Report,
        next: &'static str,
        now: i64,
    ) -> (Outcome, Vec<Effect>) {
        tracker.report("plugin-a", "account-a", "runtime", value, now, || {
            next.into()
        })
    }

    #[test]
    fn content_switch_closes_old_and_opens_new_at_observed_boundary() {
        let mut tracker = RuntimeSessions::default();
        assert!(matches!(
            apply(
                &mut tracker,
                report(1, Transition::Started, 100, "rom-a"),
                "session-a",
                100
            )
            .0,
            Outcome::Accepted { .. }
        ));
        let (_, effects) = apply(
            &mut tracker,
            report(2, Transition::ContentChanged, 120, "rom-b"),
            "session-b",
            120,
        );
        assert!(matches!(&effects[0], Effect::Close { ended_at: 120, .. }));
        assert!(
            matches!(&effects[1], Effect::Open(session) if session.started_at == 120 && session.entry_key == "install-a")
        );
    }

    #[test]
    fn concurrent_instances_are_independent_and_duplicate_evidence_is_rejected() {
        let mut tracker = RuntimeSessions::default();
        let first = report(1, Transition::Started, 100, "rom-a");
        let (accepted, _) = apply(&mut tracker, first.clone(), "session-a", 100);
        assert!(matches!(accepted, Outcome::Accepted { .. }));

        let mut duplicate = first;
        duplicate.reporter_session_id = "reporter-b".into();
        assert_eq!(
            apply(&mut tracker, duplicate, "unused", 100).0,
            Outcome::Duplicate
        );

        let mut parallel = report(1, Transition::Started, 101, "rom-a");
        parallel.runtime_instance_id = "instance-b".into();
        parallel.reporter_session_id = "reporter-b".into();
        parallel.source_observation_id = "observation-parallel".into();
        assert!(matches!(
            apply(&mut tracker, parallel, "session-b", 101).0,
            Outcome::Accepted { .. }
        ));
    }

    #[test]
    fn identification_alone_does_not_count_and_stale_or_expired_evidence_closes_at_last_time() {
        let mut tracker = RuntimeSessions::default();
        let (_, effects) = apply(
            &mut tracker,
            report(1, Transition::Identified, 100, "rom-a"),
            "unused",
            100,
        );
        assert!(effects.is_empty());
        assert_eq!(
            apply(
                &mut tracker,
                report(2, Transition::Started, 50, "rom-a"),
                "unused",
                100
            )
            .0,
            Outcome::Stale
        );
        let (_, opened) = apply(
            &mut tracker,
            report(2, Transition::Started, 101, "rom-a"),
            "session-a",
            101,
        );
        assert!(matches!(opened.first(), Some(Effect::Open(_))));
        assert!(matches!(
            tracker.expire(200).as_slice(),
            [Effect::Close { ended_at: 101, .. }]
        ));
    }

    #[test]
    fn confirmed_pause_closes_interval_and_resume_starts_a_new_one() {
        let mut tracker = RuntimeSessions::default();
        apply(
            &mut tracker,
            report(1, Transition::Started, 100, "rom-a"),
            "first",
            100,
        );
        let (_, paused) = apply(
            &mut tracker,
            report(2, Transition::Paused, 120, "rom-a"),
            "unused",
            120,
        );
        assert!(matches!(
            paused.as_slice(),
            [Effect::Close { ended_at: 120, .. }]
        ));
        let (_, resumed) = apply(
            &mut tracker,
            report(3, Transition::Resumed, 180, "rom-a"),
            "second",
            180,
        );
        assert!(matches!(resumed.as_slice(), [Effect::Open(session)] if session.started_at == 180));
    }

    #[test]
    fn new_arbiter_after_restart_never_backfills_and_disconnect_fences_last_observation() {
        let mut old = RuntimeSessions::default();
        apply(
            &mut old,
            report(1, Transition::Started, 100, "rom-a"),
            "session-a",
            100,
        );
        assert!(matches!(
            old.fence_plugin("plugin-a").as_slice(),
            [Effect::Close { ended_at: 100, .. }]
        ));

        let mut fresh = RuntimeSessions::default();
        let (_, effects) = apply(
            &mut fresh,
            report(1, Transition::Started, 500, "rom-a"),
            "session-fresh",
            500,
        );
        assert!(matches!(effects.as_slice(), [Effect::Open(session)] if session.started_at == 500));
    }

    #[test]
    fn identification_without_gameplay_confirmation_and_reused_process_identity_do_not_merge() {
        let mut tracker = RuntimeSessions::default();
        let mut identified = report(1, Transition::Identified, 1_000, "rom-a");
        identified.runtime_instance_id = "instance-before-boot".into();
        let (outcome, effects) = apply(&mut tracker, identified, "unused", 1_000);
        assert!(matches!(
            outcome,
            Outcome::Accepted {
                host_session_id: None
            }
        ));
        assert!(
            effects.is_empty(),
            "launch/content identification alone is not playtime"
        );

        let mut failed_boot = report(2, Transition::Ended, 1_010, "rom-a");
        failed_boot.runtime_instance_id = "instance-before-boot".into();
        let (outcome, effects) = apply(&mut tracker, failed_boot, "unused", 1_010);
        assert!(matches!(
            outcome,
            Outcome::Accepted {
                host_session_id: None
            }
        ));
        assert!(
            effects.is_empty(),
            "failed boot without gameplay confirmation adds no session"
        );

        let mut reused_pid = report(1, Transition::Started, 1_020, "rom-a");
        reused_pid.runtime_instance_id = "instance-after-pid-reuse".into();
        reused_pid.source_observation_id = "observation-after-reuse".into();
        let (outcome, effects) = apply(&mut tracker, reused_pid, "session-after-reuse", 1_020);
        assert!(matches!(outcome, Outcome::Accepted { .. }));
        let Effect::Open(first) = &effects[0] else {
            panic!("confirmed gameplay opens a fresh session");
        };
        assert_eq!(first.started_at, 1_020);
        assert_eq!(first.id, "session-after-reuse");
    }
}
