//! Store-neutral dispatch of provider observations through Core policy.
use super::*;
use crate::plugin_host::runtime::{ProviderRuntime, RuntimeSessionHandler};
use crate::plugin_host::services::{AccountPersistence, LibraryAccounts};
use gamenode_core::snapshot::StoreAccountState;
use gamenode_plugin_protocol::{
    models,
    proto::{
        AccountLoginOutcome, OperationDisposition, ProviderOperation, RuntimeSessionOutcome,
        RuntimeSessionReport, RuntimeSessionResult, RuntimeSessionTransition,
    },
};

/// The launch targets a provider reported for its own games, keyed by record id (task 7.7).
///
/// A target is only a candidate: nothing here starts a process, and a target that fails the shape
/// checks is dropped with a warning, leaving the record's other facts intact. An executable target
/// keeps the record's reported install folder as its root, so its path stays relative and the
/// platform adapter can still check containment and file identity.
fn detected_targets(
    provider: &str,
    records: &[gamenode_plugin_protocol::proto::DiscoveryRecord],
) -> std::collections::BTreeMap<String, gamenode_core::library::TargetSpec> {
    let mut targets = std::collections::BTreeMap::new();
    for record in records {
        let Some(detected) = record
            .target
            .as_ref()
            .and_then(|target| target.target.as_ref())
        else {
            continue;
        };
        let spec = match detected {
            gamenode_plugin_protocol::proto::detected_target::Target::Uri(uri) => {
                gamenode_core::library::TargetSpec::Uri { uri: uri.clone() }
            }
            gamenode_plugin_protocol::proto::detected_target::Target::Executable(executable) => {
                gamenode_core::library::TargetSpec::Executable {
                    root: record.install_dir.clone(),
                    path: executable.path.clone(),
                    arguments: executable.arguments.clone(),
                    working_directory: executable.working_directory.clone(),
                }
            }
        };
        if spec.validate_shape() {
            targets.insert(record.id.clone(), spec);
        } else {
            tracing::warn!(provider, id = %record.id, "ignored an invalid detected launch target");
        }
    }
    targets
}

/// This port is called only on provider storage workers. Waiting here preserves
/// the Engine's database queue order without blocking a command or UI callback.
pub(super) struct EngineAccounts(pub(super) std::sync::Weak<Shared>);

impl EngineAccounts {
    fn queued<T: Send + 'static>(
        &self,
        work: impl FnOnce(Engine, LibraryAccounts) -> Result<T, tonic::Status> + Send + 'static,
    ) -> Result<T, tonic::Status> {
        let shared = self
            .0
            .upgrade()
            .ok_or_else(|| tonic::Status::unavailable("account host unavailable"))?;
        let engine = Engine {
            shared: shared.clone(),
        };
        let accounts = LibraryAccounts(shared.ports.library.clone());
        let (send, receive) = mpsc::sync_channel(1);
        shared.storage.submit(move || {
            let _ = send.send(work(engine, accounts));
        });
        receive
            .recv()
            .map_err(|_| tonic::Status::unavailable("account storage unavailable"))?
    }
}

impl AccountPersistence for EngineAccounts {
    fn load(&self, provider: &str) -> Result<Option<models::StoreAccount>, tonic::Status> {
        let provider = provider.to_owned();
        self.queued(move |_, accounts| accounts.load(&provider))
    }

    fn save(
        &self,
        account: models::StoreAccount,
        clear_ownership: bool,
    ) -> Result<(), tonic::Status> {
        self.queued(move |engine, accounts| {
            let provider = account.provider.clone();
            let previous = clear_ownership
                .then(|| accounts.load(&provider).ok().flatten())
                .flatten();
            let result = accounts.save(account.clone(), clear_ownership);
            if clear_ownership {
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous.account_id != account.account_id)
                    && let Some(previous) = previous
                    && let Err(error) = engine
                        .shared
                        .ports
                        .achievements
                        .delete_account(&provider, &previous.account_id)
                {
                    tracing::warn!(%error, "could not delete the previous account's achievements");
                }
                engine.clear_store_catalog_memory(&provider);
                engine.clear_achievement_memory(&provider);
            }
            result
        })
    }

    fn clear(&self, provider: &str) -> Result<(), tonic::Status> {
        let provider = provider.to_owned();
        self.queued(move |engine, accounts| {
            let previous = accounts.load(&provider).ok().flatten();
            let result = accounts.clear(&provider);
            if let Some(previous) = previous
                && let Err(error) = engine
                    .shared
                    .ports
                    .achievements
                    .delete_account(&provider, &previous.account_id)
            {
                tracing::warn!(%error, "could not delete the account's achievements");
            }
            engine.clear_store_catalog_memory(&provider);
            engine.clear_achievement_memory(&provider);
            result
        })
    }
}

pub(super) fn catalog_error(error: models::CatalogError) -> gamenode_core::library::CatalogError {
    use gamenode_core::library::CatalogError as C;
    use models::CatalogError as P;
    match error {
        P::Unreachable => C::Unreachable,
        P::RateLimited => C::RateLimited,
        P::Server => C::Server,
        P::Expired => C::Expired,
        P::Privacy => C::Privacy,
        P::Changed => C::Changed,
        P::InvalidCredential => C::InvalidCredential,
        P::SharedUnavailable => C::SharedUnavailable,
    }
}

/// Keeps catalog fields within the host's bounds even if a future transport stops enforcing
/// them. Out-of-bounds fields are ignored; they never panic or reach Core.
fn catalog_fields(
    fields: std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    fields
        .into_iter()
        .filter(|(name, value)| {
            !name.is_empty()
                && name.len() <= models::MAX_CATALOG_FIELD_KEY
                && value.len() <= models::MAX_CATALOG_FIELD_VALUE
                && !name.chars().any(char::is_control)
                && !value.chars().any(char::is_control)
        })
        .take(models::MAX_CATALOG_FIELDS)
        .collect()
}

fn category(
    value: Option<Result<Vec<models::CatalogRecord>, models::CatalogError>>,
) -> Option<Result<Vec<gamenode_core::library::CatalogRecord>, gamenode_core::library::CatalogError>>
{
    value.map(|result| {
        result
            .map(|records| {
                records
                    .into_iter()
                    .map(|r| gamenode_core::library::CatalogRecord {
                        id: r.id,
                        name: r.name,
                        fields: catalog_fields(r.fields),
                    })
                    .collect()
            })
            .map_err(catalog_error)
    })
}

/// A reference to a library choice shown in the extension package review. The host resolves
/// it against the current library; paths and permission decisions never come from this input.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionReadinessSelection {
    installation_id: String,
    revision: String,
    profile_epoch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReadinessChoice {
    name: String,
    provider_id: String,
    selection: ExtensionReadinessSelection,
}

impl Engine {
    async fn authorize_runtime_context(
        &self,
        plugin_id: String,
        publisher_fingerprint: String,
        owner_key: String,
        profile_epoch: u64,
        request: gamenode_plugin_protocol::proto::RuntimeSessionContextRequest,
    ) -> Result<crate::plugin_host::runtime::RuntimeContextRevision, tonic::Status> {
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("plugin profile service unavailable"))?;
        let preference_plugin = plugin_id.clone();
        let accounting = self
            .shared
            .runtime
            .spawn_blocking(move || profiles.is_playtime_accounting_enabled(&preference_plugin))
            .await
            .map_err(|_| tonic::Status::unavailable("playtime preference worker unavailable"))?
            .map_err(|_| tonic::Status::unavailable("playtime preference unavailable"))?;
        if !accounting {
            return Err(tonic::Status::permission_denied(
                "plugin playtime accounting is disabled",
            ));
        }
        let game_id = request
            .game_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or_else(|| tonic::Status::invalid_argument("invalid game id"))?;
        if request.installation_id.is_empty()
            || request.runtime_id.is_empty()
            || request.runtime_instance_id.is_empty()
            || request.content_id.is_empty()
        {
            return Err(tonic::Status::invalid_argument(
                "runtime context is incomplete",
            ));
        }
        let library = self.shared.ports.library.clone();
        let installation_id = request.installation_id.clone();
        let runtime_id = request.runtime_id.clone();
        let content_id = request.content_id.clone();
        let owner = owner_key.clone();
        let bindings = self
            .shared
            .runtime
            .spawn_blocking(move || {
                library.runtime_content_bindings(&owner, game_id, &installation_id)
            })
            .await
            .map_err(|_| tonic::Status::unavailable("runtime binding worker unavailable"))?
            .map_err(|_| tonic::Status::unavailable("runtime binding store unavailable"))?;
        let binding_revision = bindings
            .iter()
            .find(|binding| {
                binding.runtime_id == runtime_id
                    && binding.content_id == content_id
                    && binding
                        .supported_platforms
                        .iter()
                        .any(|platform| platform == std::env::consts::OS)
            })
            .map(|binding| binding.revision)
            .ok_or_else(|| {
                tonic::Status::permission_denied("runtime content binding is not approved")
            })?;

        let library_revision = self
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|feed| feed.revision_of(&request.installation_id))
            .ok_or_else(|| {
                tonic::Status::permission_denied("installation is not in the host library")
            })?;

        let game_name = {
            let state = self.lock();
            let Some(entry) = state.library.entry(&request.installation_id) else {
                return Err(tonic::Status::permission_denied(
                    "installation is not in the library",
                ));
            };
            if state.library.match_status(entry).game_id() != Some(game_id) {
                return Err(tonic::Status::permission_denied(
                    "installation belongs to another game",
                ));
            }
            entry
                .record()
                .name
                .unwrap_or_else(|| format!("Game {game_id}"))
        };
        let api = self
            .shared
            .selected_launch_api
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                tonic::Status::unavailable("runtime context confirmation is unavailable")
            })?;
        if !api
            .confirm_runtime_context(
                plugin_id,
                publisher_fingerprint,
                profile_epoch,
                request.game_id.clone(),
                request.installation_id.clone(),
                game_name,
            )
            .await?
        {
            Err(tonic::Status::permission_denied(
                "runtime context was not approved",
            ))
        } else {
            let library = self.shared.ports.library.clone();
            let owner = owner_key;
            let installation_id = request.installation_id.clone();
            let runtime_id = request.runtime_id.clone();
            let content_id = request.content_id.clone();
            let refreshed = self
                .shared
                .runtime
                .spawn_blocking(move || {
                    library.runtime_content_bindings(&owner, game_id, &installation_id)
                })
                .await
                .map_err(|_| tonic::Status::unavailable("runtime binding worker unavailable"))?
                .map_err(|_| tonic::Status::unavailable("runtime binding store unavailable"))?;
            let current_binding_revision = refreshed
                .iter()
                .find(|binding| {
                    binding.runtime_id == runtime_id
                        && binding.content_id == content_id
                        && binding
                            .supported_platforms
                            .iter()
                            .any(|platform| platform == std::env::consts::OS)
                })
                .map(|binding| binding.revision);
            let current_library_revision = self
                .shared
                .library_feed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .and_then(|feed| feed.revision_of(&request.installation_id));
            if current_binding_revision != Some(binding_revision)
                || current_library_revision != Some(library_revision)
            {
                return Err(tonic::Status::failed_precondition(
                    "runtime target changed during confirmation",
                ));
            }
            Ok(crate::plugin_host::runtime::RuntimeContextRevision {
                library_revision,
                binding_revision,
            })
        }
    }

    fn extension_readiness_library(
        &self,
    ) -> Result<
        (
            crate::plugin_host::profile::ProfileContext,
            Arc<crate::plugin_host::library::LibraryFeed>,
        ),
        tonic::Status,
    > {
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("plugin profiles are unavailable"))?;
        let feed = self
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("the game library is unavailable"))?;
        let profile = profiles.require()?;
        if !profile.verified() {
            return Err(tonic::Status::failed_precondition(
                "a verified account is required",
            ));
        }
        Ok((profile, feed))
    }

    pub fn extension_readiness_choices(&self) -> Vec<ExtensionReadinessChoice> {
        // A profile transition publishes the new library under the engine lock.
        let _state = self.lock();
        let Ok((profile, feed)) = self.extension_readiness_library() else {
            return Vec::new();
        };
        feed.readiness_targets()
            .into_iter()
            .map(
                |(installation_id, name, provider_id, revision)| ExtensionReadinessChoice {
                    name,
                    provider_id,
                    selection: ExtensionReadinessSelection {
                        installation_id,
                        revision: revision.to_string(),
                        profile_epoch: profile.epoch().to_string(),
                    },
                },
            )
            .collect()
    }

    pub fn resolve_extension_readiness(
        &self,
        selection: ExtensionReadinessSelection,
    ) -> Result<crate::plugin_host::process::ExtensionTarget, tonic::Status> {
        let _state = self.lock();
        let (profile, feed) = self.extension_readiness_library()?;
        let stale = || tonic::Status::failed_precondition("the selected extension target changed");
        if selection.profile_epoch != profile.epoch().to_string() {
            return Err(stale());
        }
        let revision = selection.revision.parse().map_err(|_| stale())?;
        let game_id = feed
            .readiness_game(&selection.installation_id, revision)
            .ok_or_else(stale)?;
        let installation_id = selection.installation_id;
        Ok(crate::plugin_host::process::ExtensionTarget {
            context: gamenode_plugin_protocol::proto::TargetGameContext {
                game_id: game_id.to_string(),
                installation_id: installation_id.clone(),
                profile_id: profile.profile_id().to_owned(),
                revision: revision.to_string(),
            },
            profile_epoch: profile.epoch(),
            still_selected: Arc::new(move || {
                feed.readiness_game(&installation_id, revision) == Some(game_id)
            }),
        })
    }

    pub async fn available_extensions(
        &self,
        operation: crate::plugin_host::intents::Operation,
    ) -> Result<Vec<crate::plugin_host::PluginInfo>, tonic::Status> {
        self.provider_runtime()
            .ok_or_else(|| tonic::Status::unavailable("plugin runtime is unavailable"))?
            .available_extensions(operation)
            .await
    }

    pub async fn start_extension_action(
        &self,
        entry_key: &str,
        plugin_id: &str,
        operation: crate::plugin_host::intents::Operation,
    ) -> Result<(), tonic::Status> {
        let profile = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("plugin profiles are unavailable"))?
            .require()?;
        let feed = self
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("the game library is unavailable"))?;
        let (source, projected) = feed
            .projected_entry(entry_key)
            .ok_or_else(|| tonic::Status::not_found("the selected installation is unavailable"))?;
        if operation == crate::plugin_host::intents::Operation::Launch
            && source.install_state != crate::plugin_host::library::InstallState::Installed
        {
            return Err(tonic::Status::failed_precondition(
                "extension actions require an installed game",
            ));
        }
        let game_id = projected
            .game_id
            .ok_or_else(|| tonic::Status::failed_precondition("the game is not matched"))?
            .to_string();
        let target_revision = match operation {
            crate::plugin_host::intents::Operation::Launch => {
                feed.host()
                    .ok_or_else(|| tonic::Status::unavailable("the game library is unavailable"))?
                    .launch_fingerprint(entry_key.to_owned())
                    .await?
            }
            crate::plugin_host::intents::Operation::Install => 0,
            _ => {
                return Err(tonic::Status::invalid_argument(
                    "unsupported extension action",
                ));
            }
        };
        let runtime = self
            .provider_runtime()
            .ok_or_else(|| tonic::Status::unavailable("plugin runtime is unavailable"))?;
        let selected_key = entry_key.to_owned();
        let installation_id = projected.installation_id.clone();
        let still_selected = Arc::new(move || {
            feed.projected_entry(&selected_key)
                .is_some_and(|(current, entry)| {
                    entry.installation_id == installation_id
                        && current.provider_id == source.provider_id
                        && current.game_id == source.game_id
                        && current.install_dir == source.install_dir
                        && (operation != crate::plugin_host::intents::Operation::Launch
                            || current.install_state
                                == crate::plugin_host::library::InstallState::Installed)
                })
        });
        runtime
            .start_extension(
                plugin_id,
                crate::plugin_host::process::ExtensionTarget {
                    context: gamenode_plugin_protocol::proto::TargetGameContext {
                        game_id,
                        installation_id: projected.installation_id,
                        profile_id: profile.profile_id().to_owned(),
                        revision: target_revision.to_string(),
                    },
                    profile_epoch: profile.epoch(),
                    still_selected,
                },
                operation,
            )
            .await
    }

    pub fn set_provider_runtime(&self, runtime: Arc<dyn ProviderRuntime>) {
        runtime.set_account_persistence(Arc::new(EngineAccounts(Arc::downgrade(&self.shared))));
        runtime.set_runtime_session_handler(Arc::new(EngineRuntimeSessionHandler(Arc::downgrade(
            &self.shared,
        ))));
        let weak = Arc::downgrade(&self.shared);
        let background = self.shared.runtime.clone();
        runtime.set_observer(Arc::new(move || {
            let weak = weak.clone();
            background.spawn_blocking(move || {
                if let Some(shared) = weak.upgrade() {
                    Engine { shared }.publish_current();
                }
            });
        }));
        *self
            .shared
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(runtime);
    }

    /// Accepts authenticated runtime evidence after validating it against the host-owned content
    /// binding and the current library/account state. Blocking database reads run on Tokio's
    /// worker pool; only the short in-memory arbitration and effect scheduling use the engine lock.
    pub(super) async fn report_runtime_session(
        &self,
        plugin_id: String,
        owner_key: String,
        report: RuntimeSessionReport,
        context_authorized: bool,
        context_revision: Option<crate::plugin_host::runtime::RuntimeContextRevision>,
        received_at: std::time::Instant,
    ) -> RuntimeSessionResult {
        let correlation_id = report.correlation_id.clone();
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(profiles) = profiles else {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnavailable,
                "plugin profile service unavailable",
            );
        };
        let accounting_plugin_id = plugin_id.clone();
        let accounting_enabled = self
            .shared
            .runtime
            .spawn_blocking(move || profiles.is_playtime_accounting_enabled(&accounting_plugin_id))
            .await;
        match accounting_enabled {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedUnavailable,
                    "plugin playtime accounting is disabled",
                );
            }
            _ => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedUnavailable,
                    "plugin playtime preference unavailable",
                );
            }
        }
        let game_id = match report.game_id.parse::<u64>() {
            Ok(id) if id > 0 => id,
            _ => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedInvalid,
                    "invalid game id",
                );
            }
        };
        let installation_for_read = report.installation_id.clone();
        let runtime_id_for_read = report.runtime_id.clone();
        let content_id_for_read = report.content_id.clone();
        let library = self.shared.ports.library.clone();
        let bindings = match self
            .shared
            .runtime
            .spawn_blocking(move || {
                library.runtime_content_bindings(&owner_key, game_id, &installation_for_read)
            })
            .await
        {
            Ok(Ok(bindings)) => bindings,
            _ => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedUnavailable,
                    "runtime binding store unavailable",
                );
            }
        };
        let binding = bindings.iter().find(|binding| {
            binding.runtime_id == runtime_id_for_read
                && binding.content_id == content_id_for_read
                && binding
                    .supported_platforms
                    .iter()
                    .any(|platform| platform == std::env::consts::OS)
        });
        if binding.is_none() {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnauthorized,
                "content/runtime binding is not approved",
            );
        }
        if context_authorized {
            let Some(expected) = context_revision else {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedUnauthorized,
                    "approved runtime context revision is missing",
                );
            };
            let current_library_revision = self
                .shared
                .library_feed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .and_then(|feed| feed.revision_of(&report.installation_id));
            if binding.map(|binding| binding.revision) != Some(expected.binding_revision)
                || current_library_revision != Some(expected.library_revision)
            {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedStale,
                    "runtime target changed after context approval",
                );
            }
        }

        let mut state = self.lock();
        if !state.playtime.ready || state.playtime.is_stopped() {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnavailable,
                "playtime service unavailable",
            );
        }
        let Some(account) = state.library.account().map(|account| account.id.clone()) else {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnavailable,
                "account profile unavailable",
            );
        };
        let Some(entry) = state.library.entry(&report.installation_id) else {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnauthorized,
                "installation is not in the library",
            );
        };
        if state.library.match_status(entry).game_id() != Some(game_id) {
            return runtime_result(
                correlation_id,
                RuntimeSessionOutcome::RejectedUnauthorized,
                "installation belongs to another game",
            );
        }
        let process_evidence_running = state.playtime.tracker.running().contains(&entry.key);
        let source = state.library.provider(&entry.provider).map_or_else(
            || entry.provider.clone(),
            |provider| provider.source().to_owned(),
        );
        let has_counter = state
            .library
            .provider(&entry.provider)
            .is_some_and(|provider| provider.manifest().playtime.is_some());
        match runtime_report_source_conflict(
            process_evidence_running,
            state.playtime.source.uses_counter(has_counter),
        ) {
            Some(RuntimeReportSourceConflict::ProcessEvidence) => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::Duplicate,
                    "process evidence already measures this installation",
                );
            }
            Some(RuntimeReportSourceConflict::ProviderCounter) => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedUnavailable,
                    "selected playtime source is the provider counter",
                );
            }
            None => {}
        }
        let transition = match RuntimeSessionTransition::try_from(report.transition) {
            Ok(RuntimeSessionTransition::Identified) => {
                gamenode_core::tracker::runtime_sessions::Transition::Identified
            }
            Ok(RuntimeSessionTransition::Started) => {
                gamenode_core::tracker::runtime_sessions::Transition::Started
            }
            Ok(RuntimeSessionTransition::Paused) => {
                gamenode_core::tracker::runtime_sessions::Transition::Paused
            }
            Ok(RuntimeSessionTransition::Resumed) => {
                gamenode_core::tracker::runtime_sessions::Transition::Resumed
            }
            Ok(RuntimeSessionTransition::ContentChanged) => {
                gamenode_core::tracker::runtime_sessions::Transition::ContentChanged
            }
            Ok(RuntimeSessionTransition::Ended) => {
                gamenode_core::tracker::runtime_sessions::Transition::Ended
            }
            Ok(RuntimeSessionTransition::IdentificationLost) => {
                gamenode_core::tracker::runtime_sessions::Transition::IdentificationLost
            }
            _ => {
                return runtime_result(
                    correlation_id,
                    RuntimeSessionOutcome::RejectedInvalid,
                    "invalid transition",
                );
            }
        };
        let now = unix_now();
        let observation = gamenode_core::tracker::runtime_sessions::Report {
            correlation_id: report.correlation_id.clone(),
            game_id,
            installation_id: report.installation_id.clone(),
            content_id: report.content_id,
            runtime_id: report.runtime_id,
            runtime_instance_id: report.runtime_instance_id,
            reporter_session_id: report.reporter_session_id,
            sequence: report.sequence,
            observed_at: report.observed_at,
            transition,
            source_observation_id: report.source_observation_id,
            context_authorized,
            received_at,
        };
        let (outcome, effects) = state.playtime.runtime_sessions.report(
            &plugin_id,
            &account,
            &source,
            observation,
            now,
            gamenode_platform::random::session_id,
        );
        let (outcome, host_session_id, reason, accepted) = match outcome {
            gamenode_core::tracker::runtime_sessions::Outcome::Accepted { host_session_id } => (
                RuntimeSessionOutcome::Accepted,
                host_session_id.unwrap_or_default(),
                String::new(),
                true,
            ),
            gamenode_core::tracker::runtime_sessions::Outcome::Duplicate => (
                RuntimeSessionOutcome::Duplicate,
                String::new(),
                "duplicate evidence".into(),
                true,
            ),
            gamenode_core::tracker::runtime_sessions::Outcome::Invalid(reason) => (
                RuntimeSessionOutcome::RejectedInvalid,
                String::new(),
                reason.into(),
                false,
            ),
            gamenode_core::tracker::runtime_sessions::Outcome::Stale => (
                RuntimeSessionOutcome::RejectedStale,
                String::new(),
                "report is outside the freshness window".into(),
                false,
            ),
            gamenode_core::tracker::runtime_sessions::Outcome::Conflict(reason) => (
                RuntimeSessionOutcome::RejectedConflict,
                String::new(),
                reason.into(),
                false,
            ),
            gamenode_core::tracker::runtime_sessions::Outcome::Unavailable(reason) => (
                RuntimeSessionOutcome::RejectedUnavailable,
                String::new(),
                reason.into(),
                false,
            ),
        };
        self.track_effects(&mut state, runtime_track_effects(effects));
        self.publish(&mut state);
        RuntimeSessionResult {
            correlation_id,
            outcome: outcome as i32,
            host_session_id,
            accepted_sequence: if accepted { report.sequence } else { 0 },
            accepted_at: if accepted { now } else { 0 },
            reason,
            denial: None,
        }
    }

    /// Persist a per-plugin playtime preference. Disabling fences active runtime sessions at
    /// their last accepted observation while preserving all completed history.
    pub async fn set_plugin_playtime_accounting(
        &self,
        plugin_id: String,
        enabled: bool,
    ) -> Result<(), String> {
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        let persisted_plugin_id = plugin_id.clone();
        self.shared
            .runtime
            .spawn_blocking(move || {
                profiles
                    .set_playtime_accounting_enabled(&persisted_plugin_id, enabled)
                    .map_err(|error| error.message().to_owned())
            })
            .await
            .map_err(|_| "plugin accounting preference worker failed".to_owned())??;
        if !enabled {
            self.fence_runtime_sessions(&plugin_id);
        }
        Ok(())
    }

    pub async fn plugin_playtime_accounting_enabled(
        &self,
        plugin_id: String,
    ) -> Result<bool, String> {
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        self.shared
            .runtime
            .spawn_blocking(move || {
                profiles
                    .is_playtime_accounting_enabled(&plugin_id)
                    .map_err(|error| error.message().to_owned())
            })
            .await
            .map_err(|_| "plugin accounting preference worker failed".to_owned())?
    }

    fn fence_runtime_sessions(&self, plugin_id: &str) {
        let mut state = self.lock();
        let effects = state.playtime.runtime_sessions.fence_plugin(plugin_id);
        let effects = runtime_track_effects(effects);
        self.track_effects(&mut state, effects);
        self.publish(&mut state);
    }
    pub fn provider_health(&self, id: &str) -> (bool, bool) {
        self.provider_runtime()
            .map(|runtime| (runtime.available(id), runtime.connected(id)))
            .unwrap_or((false, false))
    }

    pub fn provider_availability(&self, id: &str) -> &'static str {
        self.provider_runtime()
            .map(|runtime| runtime.availability(id))
            .unwrap_or("unavailable")
    }

    pub fn managed_plugin_inventory(&self) -> Vec<crate::plugin_host::PluginInfo> {
        self.provider_runtime()
            .map(|runtime| runtime.managed_plugin_inventory())
            .unwrap_or_default()
    }

    pub(super) fn provider_runtime(&self) -> Option<Arc<dyn ProviderRuntime>> {
        self.shared
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether `id` names a provider the host loaded with an account and catalog.
    pub fn has_catalog_provider(&self, id: &str) -> bool {
        crate::commands::catalog_provider_loaded(self.provider_runtime().as_ref(), id)
    }

    pub(super) fn provider_account_views(
        &self,
        library: Option<&gamenode_core::library::LibraryView>,
        providers: &[gamenode_core::provider::Provider],
    ) -> Vec<StoreAccountView> {
        let Some(runtime) = self.provider_runtime() else {
            return vec![];
        };
        runtime
            .accounts()
            .into_iter()
            .filter_map(|provider| {
                let status = runtime.status(&provider)?;
                let available = runtime.available(&provider);
                let count =
                    library.and_then(|view| view.providers.iter().find(|p| p.id == provider));
                let shared_label = providers
                    .iter()
                    .find(|declared| declared.id() == provider)
                    .and_then(|declared| declared.shared_label())
                    .map(str::to_owned);
                let state = match status.state {
                    models::AccountState::NotConnected => StoreAccountState::NotConnected,
                    models::AccountState::Checking => StoreAccountState::Checking,
                    models::AccountState::SigningIn => StoreAccountState::SigningIn,
                    models::AccountState::Connected => StoreAccountState::Connected,
                    models::AccountState::Expired => StoreAccountState::Expired,
                    models::AccountState::Unreachable => StoreAccountState::Unreachable,
                    models::AccountState::CredentialError => StoreAccountState::CredentialError,
                };
                Some(StoreAccountView {
                    provider,
                    state,
                    account_id: status.account_id.clone(),
                    account_name: status.account_name.clone(),
                    owned_count: count.map_or(0, |c| c.owned_count),
                    shared_count: count.map_or(0, |c| c.shared_count),
                    shared_label,
                    installed_count: count.map_or(0, |c| c.installed_count),
                    last_checked_at: status.last_checked_at,
                    last_synced_at: status.last_synced_at,
                    storage: Some(match status.storage {
                        models::StorageMethod::PrivateFile => {
                            gamenode_core::StorageMethod::PrivateFile
                        }
                        models::StorageMethod::SecretService => {
                            gamenode_core::StorageMethod::SecretService
                        }
                        models::StorageMethod::WindowsCredentialManager => {
                            gamenode_core::StorageMethod::WindowsCredentialManager
                        }
                    }),
                    persist_failed: status.persist_failed,
                    catalog_error: status.catalog_error.map(catalog_error),
                    shared_error: status.shared_error.map(catalog_error),
                    can_sign_in: available && status.can_sign_in(),
                    can_focus_sign_in: available && status.can_focus_sign_in(),
                    can_cancel_sign_in: available && status.can_cancel_sign_in(),
                    can_check: available && status.can_check(),
                    can_sign_out: available && status.can_sign_out(),
                    can_remove_credential: available && status.can_remove_credential(),
                })
            })
            .collect()
    }
    pub(super) fn start_providers(&self) {
        if let Some(runtime) = self.provider_runtime() {
            for provider in runtime.accounts() {
                let engine = self.clone();
                self.shared.runtime.spawn(async move {
                    engine
                        .provider_action(provider, ProviderOperation::Check)
                        .await;
                });
            }
        }
    }
    pub async fn provider_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::SignIn)
            .await;
    }
    pub async fn provider_focus_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::FocusSignIn)
            .await;
    }
    pub async fn provider_cancel_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::CancelSignIn)
            .await;
    }
    pub async fn provider_sign_out(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::SignOut)
            .await;
    }
    pub async fn provider_check(&self, id: &str) {
        if !self
            .provider_runtime()
            .is_some_and(|r| r.accounts().iter().any(|p| p == id))
        {
            return;
        }
        if self.library_is_active() {
            self.request_catalog();
        } else {
            self.provider_action(id.to_owned(), ProviderOperation::Check)
                .await;
        }
    }

    /// Stops one plugin process for this run. The runtime cancels a pending sign-in first and
    /// waits for its bounded shutdown.
    pub async fn plugin_stop(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .provider_runtime()
            .ok_or_else(|| "the plugin host is unavailable".to_owned())?;
        if !runtime.has_provider(id) {
            return Err(format!("unknown provider: {id}"));
        }
        runtime
            .stop(id)
            .await
            .map_err(|status| status.message().to_owned())?;
        self.publish_current();
        Ok(())
    }

    /// Starts a stopped plugin with a fresh process session and restores its account.
    pub async fn plugin_start(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .provider_runtime()
            .ok_or_else(|| "the plugin host is unavailable".to_owned())?;
        if !runtime.has_provider(id) {
            return Err(format!("unknown provider: {id}"));
        }
        runtime
            .start(id)
            .await
            .map_err(|status| status.message().to_owned())?;
        self.publish_current();
        Ok(())
    }
    /// Adds or replaces one provider in the running library and requests a scan.
    ///
    /// The accepted import has already registered and enabled the package with the live host;
    /// this only makes the existing library list and scan it. A scan already running coalesces
    /// the request into one more run, so the caller never waits for provider I/O.
    pub fn add_library_provider(&self, provider: gamenode_core::provider::Provider) {
        let id = provider.id().to_owned();
        {
            let mut state = self.lock();
            state.library.put_provider(provider);
            self.publish(&mut state);
        }
        tracing::info!(provider = %id, "added an imported provider to the library");
        self.rescan();
    }

    /// Removes an uninstalled provider and its live rows from the running library.
    ///
    /// Commands pass the installed plugin id, which may differ from the provider id a package
    /// contributes (for example `dolphin-emu`); the machine resolves the store by its discovery
    /// plugin. A direct provider id is still accepted. The persisted cache keeps the rows so a
    /// later reinstall can restore them; the library view stops showing the store immediately,
    /// and a running scan no longer waits for it.
    pub fn remove_library_provider(&self, id: &str) {
        let mut state = self.lock();
        if state.library.remove_provider_for_plugin(id) || state.library.remove_provider(id) {
            self.publish(&mut state);
            tracing::info!(provider = %id, "removed an uninstalled provider from the library");
        }
    }

    fn provider_check_gate(&self, provider: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.shared
            .provider_checks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(provider.to_owned())
            .or_default()
            .clone()
    }

    async fn provider_action(&self, provider: String, operation: ProviderOperation) {
        let Some(runtime) = self.provider_runtime() else {
            return;
        };
        if !runtime.accounts().contains(&provider) {
            return;
        }
        // A startup check can report the same identity replacement as a joined
        // catalog sync. Serialize check execution AND its commit with sync so a
        // late check cannot clear the newly imported catalog. Sign-out/sign-in
        // remain free to preempt network work through host epoch invalidation.
        let _check = if operation == ProviderOperation::Check {
            Some(self.provider_check_gate(&provider).lock_owned().await)
        } else {
            None
        };
        let Ok(completion) = runtime.execute(&provider, operation).await else {
            self.publish_current();
            return;
        };
        let engine = self.clone();
        let _ = completion
            .commit(move |result, _| {
                if result.disposition == OperationDisposition::Superseded as i32 {
                    return;
                }
                if result.signed_out || result.account_changed {
                    engine.clear_store_catalog(&provider);
                }
                if result.login_outcome == Some(AccountLoginOutcome::Connected as i32)
                    || (result.account_changed
                        && result.status.as_ref().is_some_and(|s| {
                            s.state
                                == gamenode_plugin_protocol::proto::AccountConnection::Connected
                                    as i32
                        }))
                {
                    engine.request_catalog();
                }
                if result.status.as_ref().is_some_and(|status| {
                    status.state
                        == gamenode_plugin_protocol::proto::AccountConnection::Connected as i32
                }) {
                    engine.resume_achievements(&provider);
                }
                engine.publish_current();
            })
            .await;
    }
    pub(super) fn scan_provider(
        &self,
        runtime: Arc<dyn ProviderRuntime>,
        found: gamenode_core::provider::Provider,
        op: u64,
        provider: String,
    ) {
        let engine = self.clone();
        self.shared.runtime.spawn(async move {
            match runtime
                .execute(&provider, ProviderOperation::Discover)
                .await
            {
                Ok(completion) => {
                    let failed_engine = engine.clone();
                    let failed_provider = provider.clone();
                    let committed = completion
                        .commit(move |result, _| {
                            let mut scanned = None;
                            let result = if result.disposition
                                == OperationDisposition::Completed as i32
                            {
                                result
                                    .discovery
                                    .filter(|d| d.provider_id == provider)
                                    .map(|d| {
                                        // Targets are collected before the records are
                                        // flattened, so a provider's typed target can never
                                        // be mistaken for a template field.
                                        engine.library_input(LibInput::ProviderLocated {
                                            op,
                                            provider: provider.clone(),
                                            install_root: d.install_root.clone(),
                                            library_folders: d.library_folders,
                                        });
                                        let detected = detected_targets(&provider, &d.records);
                                        let records =
                                            gamenode_core::provider::record::finish_records(
                                                &provider,
                                                d.records
                                                    .into_iter()
                                                    .map(|r| gamenode_core::provider::RawRecord {
                                                        id: Some(r.id),
                                                        name: Some(r.name),
                                                        install_dir: Some(r.install_dir),
                                                        fields: r.fields.into_iter().collect(),
                                                    })
                                                    .collect(),
                                                &found.manifest().exclude_ids,
                                            );
                                        let targets = detected
                                            .into_iter()
                                            .filter(|(id, _)| {
                                                records.iter().any(|record| &record.id == id)
                                            })
                                            .collect();
                                        scanned = Some(targets);
                                        records
                                    })
                                    .ok_or_else(|| {
                                        "provider discovery returned no observations".into()
                                    })
                            } else {
                                Err("provider discovery failed".into())
                            };
                            engine.library_input(LibInput::ProviderScanned {
                                op,
                                provider,
                                result,
                                targets: scanned.unwrap_or_default(),
                                now: unix_now(),
                            });
                        })
                        .await;
                    if committed.is_err() {
                        failed_engine.library_input(LibInput::ProviderScanned {
                            op,
                            provider: failed_provider,
                            result: Err("provider discovery superseded".into()),
                            targets: Default::default(),
                            now: unix_now(),
                        });
                    }
                }
                Err(_) => engine.library_input(LibInput::ProviderScanned {
                    op,
                    provider,
                    result: Err("provider discovery unavailable".into()),
                    targets: Default::default(),
                    now: unix_now(),
                }),
            }
        });
    }
    /// Whether `provider` declares shared catalog access in its trusted manifest. A provider
    /// without the declaration may only sync owned facts.
    fn declares_shared_catalog(&self, provider: &str) -> bool {
        self.lock()
            .library
            .provider(provider)
            .is_some_and(|provider| provider.shared_label().is_some())
    }
    pub(super) async fn sync_provider_catalog(&self, op: u64, provider: String) {
        let _check = self.provider_check_gate(&provider).lock_owned().await;
        let shared_declared = self.declares_shared_catalog(&provider);
        let Some(runtime) = self.provider_runtime() else {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                shared: None,
                now: unix_now(),
            });
            return;
        };
        if runtime.status(&provider).is_some_and(|status| {
            matches!(
                status.state,
                models::AccountState::Expired
                    | models::AccountState::Unreachable
                    | models::AccountState::CredentialError
            ) || (status.state == models::AccountState::NotConnected
                && status.last_checked_at.is_some())
        }) {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                shared: None,
                now: unix_now(),
            });
            return;
        }
        let mut completion = runtime
            .execute(&provider, ProviderOperation::SyncCatalog)
            .await;
        // A sync may join the startup session check. Once that check is done,
        // request the actual catalog once instead of accepting no observations.
        let retry = completion.as_ref().is_ok_and(|c| {
            (c.catalog.owned.is_none() && c.catalog.shared.is_none()
                || c.result.disposition == OperationDisposition::Superseded as i32)
                && runtime
                    .status(&provider)
                    .is_some_and(|s| s.state == models::AccountState::Connected)
        });
        if retry {
            if let Ok(previous) = completion
                && previous.result.account_changed
            {
                let engine = self.clone();
                let provider = provider.clone();
                let _ = previous
                    .commit(move |_, _| engine.clear_store_catalog(&provider))
                    .await;
            }
            completion = runtime
                .execute(&provider, ProviderOperation::SyncCatalog)
                .await;
        }
        let engine = self.clone();
        let fallback = || {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider: provider.clone(),
                owned: Some(Err(gamenode_core::library::CatalogError::Unreachable)),
                shared: None,
                now: unix_now(),
            })
        };
        match completion {
            Ok(completion) => {
                let provider = provider.clone();
                let result = completion
                    .commit(move |result, catalog| {
                        if result.account_changed || result.signed_out {
                            engine.clear_store_catalog(&provider);
                            if !result.signed_out
                                && result.status.is_some_and(|s| {
                                    s.state
                                    == gamenode_plugin_protocol::proto::AccountConnection::Connected
                                        as i32
                                })
                            {
                                engine.request_catalog();
                            }
                            return;
                        }
                        let (owned, shared) =
                            if result.disposition == OperationDisposition::Completed as i32 {
                                (category(catalog.owned), category(catalog.shared))
                            } else {
                                (None, None)
                            };
                        if shared.is_some() && !shared_declared {
                            tracing::warn!(
                                provider = %provider,
                                "provider sent shared catalog data without declaring shared access"
                            );
                            // Defense in depth: the host rejects such data as a protocol error
                            // before it reaches the engine. Should it still arrive, fail the
                            // whole sync like the host fallback does; the previous owned and
                            // shared facts stay unchanged.
                            engine.library_input(LibInput::CatalogSynced {
                                op,
                                provider,
                                owned: Some(Err(gamenode_core::library::CatalogError::Unreachable)),
                                shared: None,
                                now: catalog.now,
                            });
                            return;
                        }
                        if owned.is_some() || shared.is_some() {
                            engine.persist_catalog(
                                provider.clone(),
                                CatalogUpdate {
                                    owned: owned.clone(),
                                    shared: shared.clone(),
                                    now: catalog.now,
                                },
                            );
                        }
                        engine.library_input(LibInput::CatalogSynced {
                            op,
                            provider,
                            owned,
                            shared,
                            now: catalog.now,
                        });
                    })
                    .await;
                if result.is_err() {
                    fallback();
                }
                self.request_import_achievements();
            }
            Err(_) => {
                fallback();
                self.request_import_achievements();
            }
        }
    }
}

fn runtime_result(
    correlation_id: String,
    outcome: RuntimeSessionOutcome,
    reason: &str,
) -> RuntimeSessionResult {
    RuntimeSessionResult {
        correlation_id,
        outcome: outcome as i32,
        host_session_id: String::new(),
        accepted_sequence: 0,
        accepted_at: 0,
        reason: reason.to_owned(),
        denial: None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeReportSourceConflict {
    ProcessEvidence,
    ProviderCounter,
}

fn runtime_report_source_conflict(
    process_evidence_running: bool,
    provider_counter_selected: bool,
) -> Option<RuntimeReportSourceConflict> {
    if process_evidence_running {
        Some(RuntimeReportSourceConflict::ProcessEvidence)
    } else if provider_counter_selected {
        Some(RuntimeReportSourceConflict::ProviderCounter)
    } else {
        None
    }
}

fn runtime_track_effects(
    effects: Vec<gamenode_core::tracker::runtime_sessions::Effect>,
) -> Vec<gamenode_core::tracker::Effect> {
    effects
        .into_iter()
        .map(|effect| match effect {
            gamenode_core::tracker::runtime_sessions::Effect::Open(session) => {
                gamenode_core::tracker::Effect::OpenSession(session)
            }
            gamenode_core::tracker::runtime_sessions::Effect::Checkpoint { id, at } => {
                gamenode_core::tracker::Effect::Checkpoint { id, at }
            }
            gamenode_core::tracker::runtime_sessions::Effect::Close {
                id,
                ended_at,
                game_id,
            } => gamenode_core::tracker::Effect::CloseSession {
                id,
                ended_at,
                game_id,
            },
        })
        .collect()
}

struct EngineRuntimeSessionHandler(std::sync::Weak<Shared>);

#[tonic::async_trait]
impl RuntimeSessionHandler for EngineRuntimeSessionHandler {
    async fn authorize_context(
        &self,
        plugin_id: String,
        publisher_fingerprint: String,
        owner_key: String,
        profile_epoch: u64,
        request: gamenode_plugin_protocol::proto::RuntimeSessionContextRequest,
    ) -> Result<crate::plugin_host::runtime::RuntimeContextRevision, tonic::Status> {
        let shared = self
            .0
            .upgrade()
            .ok_or_else(|| tonic::Status::unavailable("playtime engine unavailable"))?;
        Engine { shared }
            .authorize_runtime_context(
                plugin_id,
                publisher_fingerprint,
                owner_key,
                profile_epoch,
                request,
            )
            .await
    }

    async fn report(
        &self,
        plugin_id: String,
        owner_key: String,
        report: RuntimeSessionReport,
        context_authorized: bool,
        context_revision: Option<crate::plugin_host::runtime::RuntimeContextRevision>,
        received_at: std::time::Instant,
    ) -> RuntimeSessionResult {
        let Some(shared) = self.0.upgrade() else {
            return runtime_result(
                report.correlation_id,
                RuntimeSessionOutcome::RejectedUnavailable,
                "playtime engine unavailable",
            );
        };
        Engine { shared }
            .report_runtime_session(
                plugin_id,
                owner_key,
                report,
                context_authorized,
                context_revision,
                received_at,
            )
            .await
    }

    async fn disconnected(&self, plugin_id: String) {
        if let Some(shared) = self.0.upgrade() {
            Engine { shared }.fence_runtime_sessions(&plugin_id);
        }
    }
}

#[cfg(test)]
mod runtime_report_source_tests {
    use super::{RuntimeReportSourceConflict, runtime_report_source_conflict};

    #[test]
    fn plugin_playtime_is_not_double_counted_with_host_process_or_provider_counter_evidence() {
        assert_eq!(
            runtime_report_source_conflict(true, false),
            Some(RuntimeReportSourceConflict::ProcessEvidence)
        );
        assert_eq!(
            runtime_report_source_conflict(false, true),
            Some(RuntimeReportSourceConflict::ProviderCounter)
        );
        assert_eq!(runtime_report_source_conflict(false, false), None);
    }
}
