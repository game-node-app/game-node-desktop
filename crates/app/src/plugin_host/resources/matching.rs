//! Catalog search and match assignment of `ResourceHost`.
//!
//! Search spends the user's verified GameNode session on a title query, so it is bounded: a
//! short query, one small page and a per-plugin rate. Assign changes one of the caller's own
//! entries through the library's own match policy: the entry's revision must be current, the game
//! must be one the host knows or a search just offered, and a match the user chose is never
//! replaced. Like every mutation it needs an idempotency key and is serialized with the others.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, mutation_key, reply, request_hash, unavailable};
use super::{ResourceApi, Scope};
use crate::plugin_host::library::MatchWrite;

const MAX_QUERY_BYTES: usize = 128;
const DEFAULT_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;
/// Catalog searches one plugin may start per window, across its sessions.
pub(super) const SEARCHES_PER_WINDOW: usize = 20;
const SEARCH_WINDOW: Duration = Duration::from_secs(60);
const MAX_INSTALLATION_ID_BYTES: usize = 128;

/// Recent search times by plugin and profile epoch.
#[derive(Default)]
pub(super) struct SearchLimiter {
    recent: Mutex<HashMap<(String, u64), VecDeque<Instant>>>,
}

impl SearchLimiter {
    fn admit(&self, provider: &str, epoch: u64) -> Result<(), Status> {
        let mut recent = self.recent.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        recent.retain(|_, times| {
            times.retain(|at| now.duration_since(*at) < SEARCH_WINDOW);
            !times.is_empty()
        });
        let times = recent.entry((provider.to_owned(), epoch)).or_default();
        if times.len() >= SEARCHES_PER_WINDOW {
            return Err(Status::resource_exhausted(
                "too many catalog searches; try again in a minute",
            ));
        }
        times.push_back(now);
        Ok(())
    }
}

/// The trimmed query of a search.
fn query(text: &str) -> Result<&str, Status> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Status::invalid_argument("query is required"));
    }
    if text.len() > MAX_QUERY_BYTES {
        return Err(Status::invalid_argument("query is longer than 128 bytes"));
    }
    if text.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "query contains control characters",
        ));
    }
    Ok(text)
}

impl ResourceApi {
    pub(super) async fn search_catalog(
        &self,
        request: Request<MatchingSearchRequest>,
    ) -> Result<Response<MatchingSearchReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::MATCHING_READ)?;
        let text = query(&request.query)?;
        if !request.platform.is_empty() {
            return Err(Status::invalid_argument(
                "platform filters are not supported; leave platform empty",
            ));
        }
        if !request.cursor.is_empty() {
            return Err(Status::invalid_argument(
                "a search is a single page; leave cursor empty",
            ));
        }
        let limit = match request.page_size as usize {
            0 => DEFAULT_RESULTS,
            size => size.min(MAX_RESULTS),
        };
        let host = self.library.host().ok_or_else(unavailable)?;
        self.searches
            .admit(&caller.provider_id, caller.profile_epoch)?;
        let games = host.search_catalog(text.to_owned(), limit).await?;
        // The answer was fetched for a call that may since have lost its permission.
        self.providers
            .authorize_resource(request.context.as_ref(), cap::MATCHING_READ)?;
        Ok(Response::new(MatchingSearchReply {
            candidates: games
                .into_iter()
                .map(|game| MatchCandidate {
                    game_id: game.id.to_string(),
                    title: game.name,
                    platform: String::new(),
                    confidence: String::new(),
                    release_year: game.release_year.unwrap_or(0),
                })
                .collect(),
            next_cursor: String::new(),
        }))
    }

    pub(super) async fn assign_match(
        &self,
        request: Request<MatchingAssignRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::MATCHING_WRITE)?;
        let key = mutation_key(mutation)?;
        if !request.provider_id.is_empty() || !request.provider_game_id.is_empty() {
            return Err(Status::invalid_argument(
                "name the entry by installation_id; provider_id and provider_game_id are reserved",
            ));
        }
        if request.installation_id.is_empty() {
            return Err(Status::invalid_argument("installation_id is required"));
        }
        if request.installation_id.len() > MAX_INSTALLATION_ID_BYTES {
            return Err(Status::invalid_argument("installation_id is too long"));
        }
        let game = game_id(&request.gamenode_game_id)?
            .ok_or_else(|| Status::invalid_argument("gamenode_game_id is required"))?;
        let hash = request_hash(&[
            "match",
            &request.installation_id,
            &game.to_string(),
            &mutation.expected_revision.to_string(),
        ]);

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        // Writes never reach another provider's entries, whatever the caller may read.
        let own = Scope {
            own_provider: caller.provider_id.clone(),
            read_all: false,
            provider_filter: None,
        };
        let (entry_key, entry) = self
            .library
            .target(&own, &request.installation_id)
            .ok_or_else(|| Status::not_found("library entry not found"))?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let guard = self.guard(
            mutation.context.clone().unwrap_or_default(),
            cap::MATCHING_WRITE,
            caller.grant_revision,
        );
        let (outcome, revision, commit) = match host.assign_match(
            &entry_key,
            game,
            mutation.expected_revision,
            guard.as_ref(),
        )? {
            MatchWrite::Applied { revision } => (MutationOutcome::Applied, revision, true),
            MatchWrite::AlreadyApplied { revision } => {
                (MutationOutcome::AlreadyApplied, revision, true)
            }
            MatchWrite::Conflict { current } => (MutationOutcome::RevisionConflict, current, false),
            MatchWrite::UnknownGame => {
                return Err(Status::not_found(
                    "the game is not known to GameNode on this device; find it with MatchingSearch first",
                ));
            }
            MatchWrite::UserChoice => {
                return Err(Status::failed_precondition(
                    "the user chose this entry's match; only the user can change it",
                ));
            }
        };
        let answer = reply(&entry.installation_id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &answer);
        }
        Ok(Response::new(answer))
    }
}
