//! Bounded, serial GameNode suggestion lookups for discovered candidates.
//!
//! A search runs only with a verified session, once per new candidate title, and its answer is
//! cached on every candidate that shares the title. The queue is bounded and the worker awaits one
//! request at a time, so a large watched folder cannot flood GameNode.
use crate::engine::{Engine, State};
use gamenode_core::drm_free::Candidate;
use gamenode_core::game_api::ApiError;
use gamenode_core::library::SuggestionView;
use gamenode_core::library::suggest::{normalize_title, rank_suggestions, years_in};

const SUGGESTION_QUEUE_LIMIT: usize = 100;

impl Engine {
    /// Queues a search for every new candidate title and starts the single worker.
    pub fn request_discovery_suggestions(&self) {
        {
            let state = self.lock();
            if state.machine.verified_credential().is_none() || state.library.account().is_none() {
                return;
            }
        }
        let changed = {
            let mut state = self.lock();
            let changed = self.queue_discovery_suggestions(&mut state);
            if !changed.is_empty() {
                self.publish(&mut state);
            }
            changed
        };
        if !changed.is_empty() {
            persist_suggestions(self, changed);
        }
        self.pump_discovery_suggestions();
    }

    /// Fills the bounded queue from candidates that still need a search. Cached title answers are
    /// applied to every candidate that shares the title, even once the queue is full, so a large
    /// watched folder eventually searches every pending title without another external refresh.
    fn queue_discovery_suggestions(
        &self,
        state: &mut State,
    ) -> Vec<(String, Option<SuggestionView>)> {
        let candidates: Vec<Candidate> = state
            .local
            .candidates
            .iter()
            .filter(|candidate| !candidate.ignored)
            .cloned()
            .collect();
        let mut changed = Vec::new();
        for candidate in candidates {
            if candidate.suggestion.is_some()
                || candidate.likely_non_game
                || state
                    .local_suggestion_searched
                    .contains(&candidate.fingerprint)
                || state
                    .local_suggestion_queued
                    .contains(&candidate.fingerprint)
            {
                continue;
            }
            let title = normalize_title(&candidate.title);
            if title.is_empty() {
                continue;
            }
            if state.local_suggestion_in_flight.contains(&title) {
                continue;
            }
            if let Some(cached) = state.local_suggestion_titles.get(&title).cloned() {
                if let Some(suggestion) = cached
                    && let Some(current) = state
                        .local
                        .candidates
                        .iter_mut()
                        .find(|current| current.fingerprint == candidate.fingerprint)
                {
                    current.suggestion = Some(suggestion.clone());
                    changed.push((current.fingerprint.clone(), Some(suggestion)));
                }
                state
                    .local_suggestion_searched
                    .insert(candidate.fingerprint.clone());
                continue;
            }
            if state.local_suggestion_queue.len() >= SUGGESTION_QUEUE_LIMIT {
                continue;
            }
            let already_queued = state.local_suggestion_queue.iter().any(|queued| {
                state.local.candidates.iter().any(|current| {
                    current.fingerprint == *queued && normalize_title(&current.title) == title
                })
            });
            if already_queued {
                continue;
            }
            state
                .local_suggestion_queue
                .push_back(candidate.fingerprint.clone());
            state
                .local_suggestion_queued
                .insert(candidate.fingerprint.clone());
        }
        changed
    }

    fn pump_discovery_suggestions(&self) {
        let (credential, suggestion_account, generation, initial_cached) = {
            let mut state = self.lock();
            if state.local_suggestion_running {
                return;
            }
            let Some(credential) = state.machine.verified_credential().cloned() else {
                state.local_suggestion_queue.clear();
                state.local_suggestion_queued.clear();
                return;
            };
            let Some(suggestion_account) =
                state.library.account().map(|account| account.id.clone())
            else {
                state.local_suggestion_queue.clear();
                state.local_suggestion_queued.clear();
                return;
            };
            let cached = if state.local_suggestion_queue.is_empty() {
                self.queue_discovery_suggestions(&mut state)
            } else {
                Vec::new()
            };
            if !cached.is_empty() {
                self.publish(&mut state);
            }
            if state.local_suggestion_queue.is_empty() {
                drop(state);
                if !cached.is_empty() {
                    persist_suggestions(self, cached);
                }
                return;
            }
            state.local_suggestion_running = true;
            (
                credential,
                suggestion_account,
                state.local_generation,
                cached,
            )
        };
        if !initial_cached.is_empty() {
            persist_suggestions(self, initial_cached);
        }
        let engine = self.clone();
        let api = self.shared.ports.games.clone();
        self.shared.runtime.spawn(async move {
            loop {
                let (candidate, cached_to_persist) = {
                    let mut state = engine.lock();
                    let valid = state.local_generation == generation
                        && state
                            .library
                            .account()
                            .is_some_and(|account| account.id == suggestion_account)
                        && state.machine.verified_credential().is_some();
                    if !valid {
                        state.local_suggestion_running = false;
                        let should_pump = state.library.account().is_some()
                            && state.machine.verified_credential().is_some();
                        drop(state);
                        if should_pump {
                            engine.pump_discovery_suggestions();
                        }
                        return;
                    }
                    // Replenish only once the queue drains, so every pending title is eventually
                    // searched in a single run without scanning the whole folder per request.
                    let cached = if state.local_suggestion_queue.is_empty() {
                        let cached = engine.queue_discovery_suggestions(&mut state);
                        if !cached.is_empty() {
                            engine.publish(&mut state);
                        }
                        cached
                    } else {
                        Vec::new()
                    };
                    let Some(fingerprint) = state.local_suggestion_queue.pop_front() else {
                        state.local_suggestion_running = false;
                        drop(state);
                        if !cached.is_empty() {
                            persist_suggestions(&engine, cached);
                        }
                        return;
                    };
                    state.local_suggestion_queued.remove(&fingerprint);
                    let candidate = state
                        .local
                        .candidates
                        .iter()
                        .find(|candidate| candidate.fingerprint == fingerprint)
                        .cloned()
                        .filter(|candidate| !candidate.ignored);
                    // Claim the title in the same critical section that pops it. Marking it in
                    // flight later let a concurrent request queue the same title again and search
                    // it twice.
                    let candidate = match candidate {
                        Some(candidate)
                            if candidate.suggestion.is_some()
                                || candidate.likely_non_game
                                || state.local_suggestion_searched.contains(&fingerprint) =>
                        {
                            state.local_suggestion_searched.insert(fingerprint.clone());
                            None
                        }
                        Some(candidate) => {
                            let title_key = normalize_title(&candidate.title);
                            if title_key.is_empty()
                                || state.local_suggestion_in_flight.contains(&title_key)
                                || state.local_suggestion_titles.contains_key(&title_key)
                            {
                                // Another step owns or already answered this title; the next
                                // queue fill applies its cached answer.
                                None
                            } else {
                                state.local_suggestion_in_flight.insert(title_key.clone());
                                Some((candidate, title_key))
                            }
                        }
                        None => None,
                    };
                    (candidate, cached)
                };
                if !cached_to_persist.is_empty() {
                    persist_suggestions(&engine, cached_to_persist);
                }
                let Some((candidate, title_key)) = candidate else {
                    // Removed, ignored, answered or owned by another step. The view may have been
                    // replaced while the request was queued; a later refresh re-queues it.
                    continue;
                };
                let title = candidate.title.clone();
                let years = years_in(&format!("{} {}", candidate.title, candidate.root));
                let result = api.search_page(&credential, &title).await;
                let rejected = matches!(result, Err(ApiError::Rejected));
                let best = match &result {
                    Ok(page) => rank_suggestions(&title, &years, &page.games)
                        .into_iter()
                        .next(),
                    Err(_) => None,
                };
                enum StepOutcome {
                    Persist(Vec<(String, Option<SuggestionView>)>),
                    Stale { should_pump: bool },
                    Rejected,
                }
                let outcome = {
                    let mut state = engine.lock();
                    state.local_suggestion_in_flight.remove(&title_key);
                    let still_valid = state.local_generation == generation
                        && state
                            .library
                            .account()
                            .is_some_and(|account| account.id == suggestion_account)
                        && state.machine.verified_credential().is_some();
                    if !still_valid {
                        state.local_suggestion_running = false;
                        let should_pump = state.library.account().is_some()
                            && state.machine.verified_credential().is_some();
                        StepOutcome::Stale { should_pump }
                    } else if rejected {
                        state.local_suggestion_running = false;
                        state.local_suggestion_queue.clear();
                        state.local_suggestion_queued.clear();
                        StepOutcome::Rejected
                    } else {
                        state
                            .local_suggestion_titles
                            .insert(title_key.clone(), best.clone());
                        let mut assignments = Vec::new();
                        let mut searched = Vec::new();
                        for current in state.local.candidates.iter_mut().filter(|current| {
                            !current.ignored
                                && !current.likely_non_game
                                && current.suggestion.is_none()
                                && normalize_title(&current.title) == title_key
                        }) {
                            current.suggestion = best.clone();
                            searched.push(current.fingerprint.clone());
                            if best.is_some() {
                                assignments.push((current.fingerprint.clone(), best.clone()));
                            }
                        }
                        state.local_suggestion_searched.extend(searched);
                        if !assignments.is_empty() {
                            engine.publish(&mut state);
                        }
                        StepOutcome::Persist(assignments)
                    }
                };
                match outcome {
                    StepOutcome::Persist(assignments) => {
                        if !assignments.is_empty() {
                            persist_suggestions(&engine, assignments);
                        }
                    }
                    StepOutcome::Stale { should_pump } => {
                        if should_pump {
                            engine.pump_discovery_suggestions();
                        }
                        return;
                    }
                    StepOutcome::Rejected => {
                        engine.check_rejected(true).await;
                        return;
                    }
                }
            }
        });
    }
}

fn persist_suggestions(engine: &Engine, assignments: Vec<(String, Option<SuggestionView>)>) {
    let Some(store) = engine.shared.ports.local_games.clone() else {
        return;
    };
    engine.shared.runtime.spawn_blocking(move || {
        for (fingerprint, suggestion) in assignments {
            if let Err(error) = store.save_candidate_suggestion(&fingerprint, suggestion.as_ref()) {
                tracing::warn!(%error, "could not save a discovery suggestion");
            }
        }
    });
}
