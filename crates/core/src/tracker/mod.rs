//! Pure game process tracking. The engine supplies times and session ids and executes effects.
pub mod scripted;

use crate::playtime::Session;
use crate::ports::ProcessInfo;
use crate::provider::path::normalize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::UNIX_EPOCH;

const DAY: i64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub install_dir: String,
    pub source: String,
    pub counter: bool,
    pub game_id: Option<u64>,
    pub title: String,
}
#[derive(Clone, Debug)]
pub enum Input {
    Library {
        account: Option<String>,
        entries: Vec<Entry>,
        at: i64,
    },
    Listed {
        at: i64,
        processes: Vec<ProcessInfo>,
    },
    Exited {
        process: ProcessInfo,
        at: i64,
    },
    Tick {
        now: i64,
    },
    Stop {
        at: i64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    GameStopped {
        key: String,
        play_period: String,
        last_gameplay_exit: i64,
    },
    Watch(ProcessInfo),
    OpenSession(Session),
    Checkpoint {
        id: String,
        at: i64,
    },
    CloseSession {
        id: String,
        ended_at: i64,
        game_id: Option<u64>,
    },
    ReadCounter {
        key: String,
    },
}

type Identity = (u32, std::time::SystemTime);
struct Running {
    play_period: String,
    processes: BTreeMap<Identity, ProcessInfo>,
    session: Option<Session>,
    last_exit: i64,
}
/// Matching rules are supplied by the host, allowing both platforms to be tested anywhere.
pub struct Tracker {
    policies: BTreeMap<String, crate::drm_free::ProcessPolicy>,
    floor: i64,
    windows: bool,
    account: Option<String>,
    entries: BTreeMap<String, Entry>,
    active: BTreeMap<String, Running>,
}
impl Tracker {
    pub fn new(windows: bool) -> Self {
        Self {
            windows,
            account: None,
            policies: BTreeMap::new(),
            floor: i64::MIN,
            entries: BTreeMap::new(),
            active: BTreeMap::new(),
        }
    }
    pub fn set_policies(&mut self, policies: BTreeMap<String, crate::drm_free::ProcessPolicy>) {
        self.policies = policies;
    }
    pub fn set_floor(&mut self, at: i64) {
        self.floor = at;
    }

    pub fn running(&self) -> BTreeSet<String> {
        self.active.keys().cloned().collect()
    }

    pub fn update(&mut self, input: Input, mut next_id: impl FnMut() -> String) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            Input::Library {
                account,
                entries,
                at,
            } => {
                if account != self.account || account.is_none() {
                    if self.account.is_some() {
                        self.floor = at;
                    }
                    self.stop(at, &mut effects, &mut next_id);
                }
                self.account = account;
                let entries: BTreeMap<_, _> =
                    entries.into_iter().map(|e| (e.key.clone(), e)).collect();
                let removed: Vec<_> =
                    self.active
                        .keys()
                        .filter(|key| {
                            entries.get(*key).zip(self.entries.get(*key)).is_none_or(
                                |(new, old)| {
                                    new.install_dir != old.install_dir
                                        || new.counter != old.counter
                                        || new.source != old.source
                                },
                            )
                        })
                        .cloned()
                        .collect();
                if !removed.is_empty() {
                    self.floor = at;
                }
                for key in removed {
                    self.close(&key, at, &mut effects, &mut next_id);
                }
                self.entries = entries;
            }
            Input::Listed { at, processes } if self.account.is_some() => {
                let mut grouped: BTreeMap<String, BTreeMap<Identity, ProcessInfo>> =
                    BTreeMap::new();
                for process in processes {
                    if let Some(entry) = match_policy(
                        &self.entries,
                        &self.policies,
                        &process.exe_path,
                        self.windows,
                    ) {
                        grouped
                            .entry(entry.key.clone())
                            .or_default()
                            .insert((process.pid, process.created_at), process);
                    }
                }
                let old_keys: Vec<_> = self.active.keys().cloned().collect();
                for key in old_keys {
                    let current = grouped.get(&key);
                    let running = self.active.get_mut(&key).unwrap();
                    let missing: Vec<_> = running
                        .processes
                        .keys()
                        .filter(|id| current.is_none_or(|p| !p.contains_key(id)))
                        .copied()
                        .collect();
                    for id in missing {
                        running.processes.remove(&id);
                        running.last_exit = running.last_exit.max(at);
                    }
                    if running.processes.is_empty() && current.is_none_or(|p| p.is_empty()) {
                        let manual = self.entries[&key].source == "manual";
                        if !manual || at >= running.last_exit.saturating_add(10) {
                            let ended = running.last_exit;
                            let period = running.play_period.clone();
                            self.close(&key, ended, &mut effects, &mut next_id);
                            if manual {
                                effects.push(Effect::GameStopped {
                                    key,
                                    play_period: period,
                                    last_gameplay_exit: ended,
                                });
                            }
                        }
                    }
                }
                for (key, processes) in grouped {
                    if !self.active.contains_key(&key) {
                        let started_at = processes
                            .values()
                            .filter_map(|p| p.created_at.duration_since(UNIX_EPOCH).ok())
                            .map(|t| t.as_secs() as i64)
                            .min()
                            .unwrap_or(at)
                            .min(at)
                            .max(self.floor);
                        let session = self.open(&key, started_at, &mut effects, &mut next_id);
                        self.active.insert(
                            key.clone(),
                            Running {
                                play_period: if self.entries[&key].source == "manual" {
                                    next_id()
                                } else {
                                    String::new()
                                },
                                processes: BTreeMap::new(),
                                session,
                                last_exit: i64::MIN,
                            },
                        );
                    }
                    let running = self.active.get_mut(&key).unwrap();
                    for (id, process) in processes {
                        if let std::collections::btree_map::Entry::Vacant(slot) =
                            running.processes.entry(id)
                        {
                            effects.push(Effect::Watch(process.clone()));
                            slot.insert(process);
                        }
                    }
                }
            }
            Input::Exited { process, at } => {
                let id = (process.pid, process.created_at);
                let key = self
                    .active
                    .iter()
                    .find(|(_, r)| r.processes.contains_key(&id))
                    .map(|(k, _)| k.clone());
                if let Some(key) = key {
                    let running = self.active.get_mut(&key).unwrap();
                    running.processes.remove(&id);
                    running.last_exit = running.last_exit.max(at);
                    if running.processes.is_empty() && self.entries[&key].source != "manual" {
                        self.close(&key, at, &mut effects, &mut next_id);
                    }
                }
            }
            Input::Tick { now } => {
                let keys: Vec<_> = self.active.keys().cloned().collect();
                for key in keys {
                    let now = if self.active[&key].processes.is_empty() {
                        self.active[&key].last_exit
                    } else {
                        now
                    };
                    self.checkpoint(&key, now, &mut effects, &mut next_id);
                }
            }
            Input::Stop { at } => {
                self.floor = at;
                self.stop(at, &mut effects, &mut next_id);
                self.account = None;
            }
            _ => {}
        }
        effects
    }
    fn open(
        &self,
        key: &str,
        at: i64,
        effects: &mut Vec<Effect>,
        next_id: &mut impl FnMut() -> String,
    ) -> Option<Session> {
        let entry = &self.entries[key];
        if entry.counter {
            return None;
        }
        let row = Session {
            id: next_id(),
            account_id: self.account.clone()?,
            entry_key: key.into(),
            game_id: entry.game_id,
            source: entry.source.clone(),
            platform_id: 6,
            started_at: at,
            checkpoint_at: at,
            ended_at: None,
        };
        effects.push(Effect::OpenSession(row.clone()));
        Some(row)
    }
    fn checkpoint(
        &mut self,
        key: &str,
        now: i64,
        effects: &mut Vec<Effect>,
        next_id: &mut impl FnMut() -> String,
    ) {
        while let Some(session) = self.active.get(key).and_then(|r| r.session.as_ref()) {
            let boundary = session.started_at.saturating_add(DAY);
            if now < boundary {
                break;
            }
            effects.push(Effect::CloseSession {
                id: session.id.clone(),
                ended_at: boundary,
                game_id: self.entries[key].game_id,
            });
            let next = self.open(key, boundary, effects, next_id);
            self.active.get_mut(key).unwrap().session = next;
        }
        if let Some(session) = self.active.get_mut(key).and_then(|r| r.session.as_mut())
            && now.saturating_sub(session.checkpoint_at) >= 60
        {
            session.checkpoint_at = now;
            effects.push(Effect::Checkpoint {
                id: session.id.clone(),
                at: now,
            });
        }
    }
    fn close(
        &mut self,
        key: &str,
        at: i64,
        effects: &mut Vec<Effect>,
        next_id: &mut impl FnMut() -> String,
    ) {
        let at = at.max(self.active[key].last_exit);
        self.checkpoint(key, at, effects, next_id);
        let running = self.active.remove(key).unwrap();
        if let Some(session) = running.session {
            effects.push(Effect::CloseSession {
                id: session.id,
                ended_at: at,
                game_id: self.entries[key].game_id,
            });
        } else {
            effects.push(Effect::ReadCounter { key: key.into() });
        }
    }
    fn stop(&mut self, at: i64, effects: &mut Vec<Effect>, next_id: &mut impl FnMut() -> String) {
        for key in self.running() {
            self.close(&key, at, effects, next_id);
        }
    }
}

/// Deepest folder wins; the executable must be below a separator, never merely share a prefix.
pub fn match_entry<'a>(
    entries: &'a BTreeMap<String, Entry>,
    exe: &str,
    windows: bool,
) -> Option<&'a Entry> {
    let compare = |path: &str| {
        let path = normalize(path);
        if windows { path.to_lowercase() } else { path }
    };
    let exe = compare(exe);
    entries
        .values()
        .filter_map(|entry| {
            let dir = compare(&entry.install_dir);
            if dir.is_empty() {
                return None;
            }
            let prefix = if dir.ends_with(['/', '\\']) {
                dir.clone()
            } else {
                format!("{dir}{}", if windows { '\\' } else { '/' })
            };
            (exe.starts_with(&prefix) && exe.len() > prefix.len()).then_some((dir.len(), entry))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, entry)| entry)
}
fn match_policy<'a>(
    entries: &'a BTreeMap<String, Entry>,
    policies: &BTreeMap<String, crate::drm_free::ProcessPolicy>,
    exe: &str,
    windows: bool,
) -> Option<&'a Entry> {
    let compare = |p: &str| {
        let p = normalize(p);
        if windows { p.to_lowercase() } else { p }
    };
    let executable = compare(exe);
    entries
        .values()
        .filter_map(|entry| {
            let root = compare(&entry.install_dir);
            let prefix = format!(
                "{}{}",
                root.trim_end_matches(['/', '\\']),
                if windows { '\\' } else { '/' }
            );
            if !executable.starts_with(&prefix) {
                return None;
            }
            let explicit = match policies.get(&entry.key) {
                Some(crate::drm_free::ProcessPolicy::Ignored) => return None,
                Some(crate::drm_free::ProcessPolicy::Explicit { gameplay, .. }) => {
                    if !gameplay.iter().any(|p| compare(p) == executable) {
                        return None;
                    }
                    true
                }
                _ if entry.source == "manual" => return None,
                _ => false,
            };
            Some((explicit, root.len(), entry))
        })
        .max_by(|a, b| {
            (a.0, a.1)
                .cmp(&(b.0, b.1))
                .then_with(|| b.2.key.cmp(&a.2.key))
        })
        .map(|(_, _, e)| e)
}

#[cfg(test)]
mod tests;
