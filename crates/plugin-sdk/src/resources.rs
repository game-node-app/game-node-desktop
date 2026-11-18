//! Convenience calls for the library resources of API 1.6: library reads, display names,
//! favorites, catalog matching and covers.
//!
//! Each method builds exactly the request a hand-written raw `ResourceHost` call would and
//! returns the host's answer unchanged, so the two are interchangeable; a service test in the
//! desktop repeats every call both ways and compares them. The helpers add no authority: they use
//! the session's `resource_context`, and the host still checks every grant, scope and revision.
//! Errors are the host's own [`tonic::Status`], whose codes the permissions document explains
//! (`NOT_FOUND` for a foreign or unknown entry, `FAILED_PRECONDITION` for a choice the user made,
//! `RESOURCE_EXHAUSTED` for a rate or byte budget, `UNAVAILABLE` when GameNode cannot answer).
//!
//! # Writing safely
//!
//! Every write needs an idempotency key and, for entries, the revision you last read:
//!
//! * Make one key per intended change with [`idempotency_key`] and **reuse it for retries** of that
//!   change after a timeout or lost connection; the host then returns the original outcome
//!   instead of applying it twice. A new change gets a new key.
//! * [`Outcome::RevisionConflict`] is a normal answer, not an error: the entry changed since you
//!   read it. Read it again ([`Resources::library_get`]) and decide whether the change still
//!   applies, using the new revision. The host does not remember conflicts, so the key may be
//!   reused with the new revision.
//! * [`Outcome::AlreadyApplied`] means the entry already had that value; nothing was written.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tonic::{Request, Status, Streaming};

use crate::proto::resource_host_client::ResourceHostClient;
use crate::proto::*;
use crate::{MAX_MESSAGE_BYTES, SERVICE_TIMEOUT};

/// The most entries the host returns in one page.
pub const MAX_PAGE_SIZE: u32 = 200;
/// How many times [`Resources::library_entries`] and [`Resources::favorite_games`] restart a
/// list the library changed under them before giving up.
const LIST_RESTARTS: usize = 3;
/// The only display field a plugin may write.
const DISPLAY_NAME: &str = "display_name";

/// A consistent walk of the library: the entries and the library revision they were read at,
/// which is where [`Resources::library_watch`] should resume from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub entries: Vec<LibraryEntry>,
    pub revision: u64,
}

/// What a write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The value changed. `Written::revision` is the new revision.
    Applied,
    /// The entry already had this value; nothing was written.
    AlreadyApplied,
    /// The entry changed since the revision you gave; nothing was written and
    /// `Written::revision` is the current one.
    RevisionConflict,
}

/// The host's answer to a write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// The installation id (or the game id, for favorites) the write was about.
    pub resource_id: String,
    /// The resource's revision after the call, or the current one after a conflict.
    pub revision: u64,
    pub outcome: Outcome,
}

/// A proposed, typed custom launch recipe. Choose one direct target: `executable_handle` or `uri`;
/// both may be empty when a host-issued `runtime_handle` wraps the detected target. URI recipes
/// may include argv, cwd and environment changes, but the host/OS URI adapter can explicitly
/// reject customizations it cannot apply. A runtime wrapper cannot be combined with a URI target.
/// Environment set/unset is a delta applied to the child process by the host. Handles are opaque
/// host-issued resource identifiers; they are never interpreted by the SDK as filesystem paths.
/// The host validates the recipe and its handles before saving it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomLaunchRecipe {
    pub game_id: String,
    pub installation_id: String,
    /// Existing content identity selected by the host; execution cannot replace it.
    pub content_id: String,
    pub expected_revision: u64,
    pub executable_handle: String,
    pub uri: String,
    pub argv: Vec<String>,
    pub working_directory_handle: String,
    pub environment_set: std::collections::BTreeMap<String, String>,
    pub environment_unset: Vec<String>,
    pub runtime_handle: String,
}

/// A runtime executable approved on the host and shared by games that use the same interpreter.
/// The handle remains opaque: plugins provide it to registration and custom-launch requests but
/// must never interpret it as a path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeSpec {
    pub name: String,
    pub version: String,
    pub executable_handle: String,
    pub supported_formats: Vec<String>,
    pub extensions: std::collections::HashMap<String, ExtensionPayload>,
    pub platform: String,
    pub argument_template: Vec<RuntimeArgumentTemplate>,
}

/// One ordered runtime argument. `ContentPath` expands to the selected content path as one
/// argv element; literal values are never parsed as shell text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeArgumentTemplate {
    Literal(String),
    ContentPath,
}

/// One content-to-runtime binding. The game and installation IDs preserve GameNode identity;
/// content and runtime handles are opaque host-issued references.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeContentBinding {
    /// Empty when registering a new content item; set to the existing ID when updating its
    /// locator after a move or rename.
    pub content_id: String,
    pub game_id: String,
    pub installation_id: String,
    pub content_handle: String,
    pub content_kind: String,
    pub runtime_id: String,
    pub supported_platforms: Vec<String>,
}

fn runtime_record(spec: RuntimeSpec, id: &str, revision: u64) -> RuntimeRecord {
    RuntimeRecord {
        id: id.to_owned(),
        name: spec.name,
        version: spec.version,
        executable_handle: spec.executable_handle,
        supported_formats: spec.supported_formats,
        extensions: spec.extensions,
        revision,
        observed_at: 0,
        platform: spec.platform,
        argument_template: spec
            .argument_template
            .into_iter()
            .map(|argument| crate::proto::RuntimeArgumentTemplate {
                value: Some(match argument {
                    RuntimeArgumentTemplate::Literal(value) => {
                        crate::proto::runtime_argument_template::Value::Literal(value)
                    }
                    RuntimeArgumentTemplate::ContentPath => {
                        crate::proto::runtime_argument_template::Value::ContentPath(
                            ContentPathArgument {},
                        )
                    }
                }),
            })
            .collect(),
    }
}

fn runtime_register_request(
    context: &ServiceContext,
    spec: RuntimeSpec,
    key: &str,
    expected_revision: u64,
) -> RuntimeRegisterRequest {
    RuntimeRegisterRequest {
        mutation: Some(MutationContext {
            context: Some(context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }),
        runtime: Some(runtime_record(spec, "", 0)),
    }
}

fn runtime_update_request(
    context: &ServiceContext,
    runtime_id: &str,
    spec: RuntimeSpec,
    field_mask: Vec<String>,
    key: &str,
    expected_revision: u64,
) -> RuntimeUpdateRequest {
    RuntimeUpdateRequest {
        mutation: Some(MutationContext {
            context: Some(context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }),
        runtime: Some(runtime_record(spec, runtime_id, expected_revision)),
        field_mask,
    }
}

fn runtime_content_request(
    context: &ServiceContext,
    binding: RuntimeContentBinding,
    key: &str,
    expected_revision: u64,
) -> ContentRegistrationRequest {
    ContentRegistrationRequest {
        mutation: Some(MutationContext {
            context: Some(context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }),
        content_id: binding.content_id,
        game_id: binding.game_id,
        installation_id: binding.installation_id,
        content_handle: binding.content_handle,
        content_kind: binding.content_kind,
        runtime_id: binding.runtime_id,
        supported_platforms: binding.supported_platforms,
    }
}

#[cfg(test)]
mod runtime_sdk_tests {
    use super::*;

    fn runtime() -> RuntimeSpec {
        RuntimeSpec {
            name: "Example Emulator".into(),
            version: "1.2".into(),
            executable_handle: "approved-runtime-handle".into(),
            supported_formats: vec!["sfc".into(), "smc".into()],
            extensions: Default::default(),
            platform: "linux".into(),
            argument_template: vec![
                RuntimeArgumentTemplate::Literal("--game".into()),
                RuntimeArgumentTemplate::ContentPath,
            ],
        }
    }

    #[test]
    fn registration_sends_host_issued_handle_and_expected_revision() {
        let context = ServiceContext {
            provider_id: "runtime-plugin".into(),
            ..Default::default()
        };
        let request = runtime_register_request(&context, runtime(), "register-key", 0);
        let mutation = request.mutation.unwrap();
        let runtime = request.runtime.unwrap();

        assert_eq!(mutation.context.as_ref(), Some(&context));
        assert_eq!(mutation.idempotency_key, "register-key");
        assert_eq!(mutation.expected_revision, 0);
        assert!(runtime.id.is_empty());
        assert_eq!(runtime.name, "Example Emulator");
        assert_eq!(runtime.executable_handle, "approved-runtime-handle");
        assert_eq!(runtime.supported_formats, ["sfc", "smc"]);
        assert_eq!(runtime.platform, "linux");
        assert_eq!(runtime.argument_template.len(), 2);
        assert!(matches!(
            runtime.argument_template[0].value.as_ref(),
            Some(crate::proto::runtime_argument_template::Value::Literal(value))
                if value == "--game"
        ));
        assert!(matches!(
            runtime.argument_template[1].value.as_ref(),
            Some(crate::proto::runtime_argument_template::Value::ContentPath(
                _
            ))
        ));
        assert_eq!(runtime.revision, 0);
        assert_eq!(runtime.observed_at, 0);
    }

    #[test]
    fn update_and_content_binding_keep_game_and_installation_identity() {
        let context = ServiceContext::default();
        let update = runtime_update_request(
            &context,
            "runtime-1",
            runtime(),
            vec!["version".into()],
            "update-key",
            7,
        );
        let updated = update.runtime.unwrap();
        assert_eq!(updated.id, "runtime-1");
        assert_eq!(updated.revision, 7);
        assert_eq!(update.field_mask, ["version"]);
        assert_eq!(update.mutation.unwrap().expected_revision, 7);

        let binding = runtime_content_request(
            &context,
            RuntimeContentBinding {
                content_id: String::new(),
                game_id: "620".into(),
                installation_id: "steam:620".into(),
                content_handle: "approved-content-handle".into(),
                content_kind: "rom".into(),
                runtime_id: "runtime-1".into(),
                supported_platforms: vec!["linux".into()],
            },
            "bind-key",
            3,
        );
        assert_eq!(binding.game_id, "620");
        assert!(binding.content_id.is_empty());
        assert_eq!(binding.installation_id, "steam:620");
        assert_eq!(binding.content_handle, "approved-content-handle");
        assert_eq!(binding.runtime_id, "runtime-1");
        assert_eq!(binding.supported_platforms, ["linux"]);
        assert_eq!(binding.mutation.unwrap().expected_revision, 3);

        let moved_binding = runtime_content_request(
            &context,
            RuntimeContentBinding {
                content_id: "content-existing".into(),
                game_id: "620".into(),
                installation_id: "steam:620".into(),
                content_handle: "approved-content-handle-after-move".into(),
                content_kind: "rom".into(),
                runtime_id: "runtime-1".into(),
                supported_platforms: vec!["linux".into()],
            },
            "move-key",
            4,
        );
        assert_eq!(moved_binding.content_id, "content-existing");
        assert_eq!(moved_binding.installation_id, "steam:620");
        assert_eq!(
            moved_binding.content_handle,
            "approved-content-handle-after-move"
        );
    }
}

impl Written {
    /// Whether the entry now has the requested value, whether this call set it or not.
    pub fn is_done(&self) -> bool {
        matches!(self.outcome, Outcome::Applied | Outcome::AlreadyApplied)
    }
}

impl TryFrom<MutationReply> for Written {
    type Error = Status;

    fn try_from(reply: MutationReply) -> Result<Self, Status> {
        let outcome = match MutationOutcome::try_from(reply.outcome) {
            Ok(MutationOutcome::Applied) => Outcome::Applied,
            Ok(MutationOutcome::AlreadyApplied) => Outcome::AlreadyApplied,
            Ok(MutationOutcome::RevisionConflict) => Outcome::RevisionConflict,
            _ => return Err(Status::internal("the host sent an unknown outcome")),
        };
        Ok(Self {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome,
        })
    }
}

/// A cover read back from the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
    pub bytes: Vec<u8>,
    /// `image/jpeg` or `image/png`.
    pub media_type: String,
    /// The entry's artwork revision: 0 while the cover is the game's own, not a plugin's.
    pub revision: u64,
}

/// A key for one intended change. Reuse it when retrying that change; use a new one for the
/// next. At most 128 bytes and free of control characters, as the host requires.
pub fn idempotency_key() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    format!(
        "sdk-{:x}-{nanos:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn timed<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(SERVICE_TIMEOUT);
    request
}

/// Typed access to the library resources, bound to one session's `resource_context`. Get it from
/// [`HostClient::resources`](crate::HostClient::resources). Cheap to clone.
#[derive(Clone)]
pub struct Resources {
    pub(crate) client: ResourceHostClient<tonic::transport::Channel>,
    pub(crate) context: ServiceContext,
}

impl Resources {
    pub(crate) fn new(channel: tonic::transport::Channel, context: ServiceContext) -> Self {
        Self {
            client: ResourceHostClient::new(channel)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
            context,
        }
    }

    pub(crate) fn mutation(&self, key: &str, expected_revision: u64) -> MutationContext {
        MutationContext {
            context: Some(self.context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }
    }

    fn custom_launch_proposal(
        context: &ServiceContext,
        recipe: &CustomLaunchRecipe,
        user_intent_id: &str,
        key: &str,
    ) -> CustomLaunchRequest {
        CustomLaunchRequest {
            context: Some(context.clone()),
            game_id: recipe.game_id.clone(),
            installation_id: recipe.installation_id.clone(),
            content_id: recipe.content_id.clone(),
            expected_revision: recipe.expected_revision,
            user_intent_id: user_intent_id.to_owned(),
            idempotency_key: key.to_owned(),
            executable_handle: recipe.executable_handle.clone(),
            argv: recipe.argv.clone(),
            working_directory_handle: recipe.working_directory_handle.clone(),
            environment_set: recipe.environment_set.clone().into_iter().collect(),
            environment_unset: recipe.environment_unset.clone(),
            runtime_handle: recipe.runtime_handle.clone(),
            recipe_id: String::new(),
            recipe_revision: 0,
            uri: recipe.uri.clone(),
        }
    }

    fn custom_launch_execution(
        context: &ServiceContext,
        game_id: &str,
        installation_id: &str,
        expected_revision: u64,
        recipe_id: &str,
        recipe_revision: u64,
        user_intent_id: &str,
        key: &str,
    ) -> CustomLaunchRequest {
        CustomLaunchRequest {
            context: Some(context.clone()),
            game_id: game_id.to_owned(),
            installation_id: installation_id.to_owned(),
            content_id: String::new(),
            expected_revision,
            user_intent_id: user_intent_id.to_owned(),
            idempotency_key: key.to_owned(),
            executable_handle: String::new(),
            argv: Vec::new(),
            working_directory_handle: String::new(),
            environment_set: Default::default(),
            environment_unset: Vec::new(),
            runtime_handle: String::new(),
            recipe_id: recipe_id.to_owned(),
            recipe_revision,
            uri: String::new(),
        }
    }

    fn custom_launch_removal(
        context: &ServiceContext,
        game_id: &str,
        recipe_id: &str,
        recipe_revision: u64,
        key: &str,
    ) -> CustomLaunchRemoveRequest {
        CustomLaunchRemoveRequest {
            mutation: Some(MutationContext {
                context: Some(context.clone()),
                operation_id: String::new(),
                idempotency_key: key.to_owned(),
                expected_revision: recipe_revision,
            }),
            game_id: game_id.to_owned(),
            recipe_id: recipe_id.to_owned(),
        }
    }

    fn page(&self, page_size: u32, cursor: &str) -> PageRequest {
        PageRequest {
            context: Some(self.context.clone()),
            cursor: cursor.to_owned(),
            page_size,
            common_data_categories: None,
        }
    }

    // ---- Runtimes and content bindings (`game.runtime.*`) ----

    /// One page of the caller's private runtime catalog by default. Supplying a selector is an
    /// explicit request for common runtime records; the host still requires the matching read
    /// grant and enforces its scope.
    pub async fn runtimes_page(
        &self,
        page_size: u32,
        cursor: &str,
        shared_records: Option<CommonDataSelector>,
    ) -> Result<RuntimesListReply, Status> {
        let mut page = self.page(page_size, cursor);
        page.common_data_categories = shared_records;
        self.client
            .clone()
            .runtimes_list(timed(DomainListRequest {
                page: Some(page),
                kind: String::new(),
                owner_filter: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// One runtime record. With no selector, the host returns only records visible to this plugin.
    pub async fn runtime_get(
        &self,
        runtime_id: &str,
        shared_records: Option<CommonDataSelector>,
    ) -> Result<RuntimeRecord, Status> {
        let reply = self
            .client
            .clone()
            .runtimes_get(timed(DomainGetRequest {
                context: Some(self.context.clone()),
                resource_id: runtime_id.to_owned(),
                expected_revision: 0,
                shared_records,
            }))
            .await?
            .into_inner();
        reply
            .runtime
            .ok_or_else(|| Status::internal("the host sent no runtime record"))
    }

    /// Runtime catalog events after `after_revision` (0: from now on). A stale position is
    /// `FAILED_PRECONDITION`; list again and resume from that reply's revision.
    pub async fn runtimes_watch(
        &self,
        after_revision: u64,
        shared_records: Option<CommonDataSelector>,
    ) -> Result<Streaming<RuntimeRecordEvent>, Status> {
        self.client
            .clone()
            .runtimes_watch(DomainWatchRequest {
                context: Some(self.context.clone()),
                after_revision,
                resource_id: String::new(),
                shared_records,
            })
            .await
            .map(tonic::Response::into_inner)
    }

    /// Register an already installed runtime. This call does not install or download software;
    /// `executable_handle` must be a host-issued approved resource handle.
    pub async fn runtime_register(
        &self,
        runtime: RuntimeSpec,
        expected_revision: u64,
        key: &str,
    ) -> Result<RuntimeMutationReply, Status> {
        self.client
            .clone()
            .runtimes_register(timed(runtime_register_request(
                &self.context,
                runtime,
                key,
                expected_revision,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Update selected fields of a runtime owned by this plugin using its last known revision.
    pub async fn runtime_update(
        &self,
        runtime_id: &str,
        runtime: RuntimeSpec,
        field_mask: impl IntoIterator<Item = impl Into<String>>,
        expected_revision: u64,
        key: &str,
    ) -> Result<RuntimeMutationReply, Status> {
        self.client
            .clone()
            .runtimes_update(timed(runtime_update_request(
                &self.context,
                runtime_id,
                runtime,
                field_mask.into_iter().map(Into::into).collect(),
                key,
                expected_revision,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Remove an owned runtime. Runtime removal does not imply content, game, save or history
    /// deletion; callers must handle the resulting unbound installations explicitly.
    pub async fn runtime_remove(
        &self,
        runtime_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .runtimes_remove(timed(DomainRemoveRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                resource_id: runtime_id.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    /// Bind one content item to an existing GameNode game and installation. The content handle
    /// and runtime ID are opaque; this operation does not create a duplicate game identity.
    pub async fn register_runtime_content(
        &self,
        binding: RuntimeContentBinding,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .games_register_content(timed(runtime_content_request(
                &self.context,
                binding,
                key,
                expected_revision,
            )))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Library reads (`library.read`, or `library.read.all` for other providers) ----

    /// One page of the library. An empty `provider_id` means the caller's own provider; naming
    /// another needs `library.read.all`. Pass the previous reply's `next_cursor` to continue; a
    /// cursor goes stale when the library changes (`FAILED_PRECONDITION`), and the list then
    /// restarts. Most plugins want [`library_entries`](Self::library_entries).
    pub async fn library_page(
        &self,
        provider_id: &str,
        search: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<LibraryListReply, Status> {
        self.client
            .clone()
            .library_list(timed(LibraryListRequest {
                page: Some(self.page(page_size, cursor)),
                provider_id: provider_id.to_owned(),
                search: search.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every entry of `provider_id` (empty: the caller's own), following the cursors. See
    /// [`library_snapshot`](Self::library_snapshot).
    pub async fn library_entries(&self, provider_id: &str) -> Result<Vec<LibraryEntry>, Status> {
        self.library_snapshot(provider_id)
            .await
            .map(|snapshot| snapshot.entries)
    }

    /// Every entry of `provider_id` (empty: the caller's own) and the library revision they were
    /// read at, following the cursors. If the library changes during the walk the list restarts,
    /// up to three times, so the result is one consistent snapshot; after that the last
    /// `FAILED_PRECONDITION` is returned. Resume a watch from `revision`.
    pub async fn library_snapshot(&self, provider_id: &str) -> Result<LibrarySnapshot, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut entries = Vec::new();
            let mut cursor = String::new();
            loop {
                match self
                    .library_page(provider_id, "", MAX_PAGE_SIZE, &cursor)
                    .await
                {
                    Ok(page) => {
                        entries.extend(page.entries);
                        if page.next_cursor.is_empty() {
                            return Ok(LibrarySnapshot {
                                entries,
                                revision: page.revision,
                            });
                        }
                        cursor = page.next_cursor;
                    }
                    // Only a cursor can go stale; a refusal of the first page is a real error.
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// One of the caller's own entries by its `installation_id`.
    pub async fn library_get(&self, installation_id: &str) -> Result<LibraryEntry, Status> {
        let reply = self
            .client
            .clone()
            .library_get(timed(LibraryGetRequest {
                context: Some(self.context.clone()),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                common_data_categories: None,
            }))
            .await?
            .into_inner();
        reply
            .entry
            .ok_or_else(|| Status::internal("the host sent no entry"))
    }

    /// Library changes after `after_revision` (0: from now on), as the host's own event stream.
    /// A position the host no longer retains is `FAILED_PRECONDITION`: list again, then watch
    /// from the list's revision.
    pub async fn library_watch(
        &self,
        provider_id: &str,
        after_revision: u64,
    ) -> Result<Streaming<LibraryEvent>, Status> {
        self.client
            .clone()
            .library_watch(LibraryWatchRequest {
                context: Some(self.context.clone()),
                after_revision,
                provider_id: provider_id.to_owned(),
                common_data_categories: None,
            })
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Display name (`library.metadata.write`) ----

    /// Sets the display name of an own entry, or restores the detected name with `None`.
    /// `expected_revision` is the entry's `metadata_revision`.
    pub async fn set_display_name(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        display_name: Option<&str>,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .metadata_update(timed(MetadataUpdateRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                display_fields: [(
                    DISPLAY_NAME.to_owned(),
                    display_name.unwrap_or_default().to_owned(),
                )]
                .into(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Favorites (`favorites.read`, `favorites.write`) ----

    /// The favorite state of every GameNode game matched by the caller's own entries, as
    /// `(game_id, favorite)`, following the cursors like [`library_entries`](Self::library_entries).
    pub async fn favorite_games(&self) -> Result<Vec<(String, bool)>, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut games = Vec::new();
            let mut cursor = String::new();
            loop {
                let reply = self
                    .client
                    .clone()
                    .favorites_list(timed(FavoritesListRequest {
                        page: Some(self.page(MAX_PAGE_SIZE, &cursor)),
                        profile_id: String::new(),
                    }))
                    .await;
                match reply {
                    Ok(page) => {
                        let page = page.into_inner();
                        games.extend(page.entries.into_iter().map(|e| (e.game_id, e.favorite)));
                        if page.next_cursor.is_empty() {
                            return Ok(games);
                        }
                        cursor = page.next_cursor;
                    }
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// Marks or unmarks a GameNode game that one of the caller's own entries is matched to. A
    /// favorite has no revision: this is a desired-state write.
    pub async fn set_favorite(
        &self,
        game_id: &str,
        favorite: bool,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .favorites_set(timed(FavoritesSetRequest {
                mutation: Some(self.mutation(key, 0)),
                profile_id: String::new(),
                game_id: game_id.to_owned(),
                favorite,
            }))
            .await?
            .into_inner()
            .try_into()
    }

    /// Proposes a recipe for later custom execution. This never calls standard `game.launch` and
    /// does not start a process; the host validates the typed handles and permission separately.
    pub async fn propose_custom_launch(
        &self,
        recipe: &CustomLaunchRecipe,
        user_intent_id: &str,
        key: &str,
    ) -> Result<CustomLaunchReply, Status> {
        self.client
            .clone()
            .games_propose_custom_launch(timed(Self::custom_launch_proposal(
                &self.context,
                recipe,
                user_intent_id,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Requests execution of a previously proposed recipe revision. The request carries no
    /// replacement executable, argv, cwd, runtime or environment payload, so the host can only
    /// execute the immutable recipe it approved. A denied `game.custom_launch` grant stays a
    /// `PERMISSION_DENIED` error; there is no fallback to standard launch.
    pub async fn request_custom_launch(
        &self,
        game_id: &str,
        installation_id: &str,
        expected_revision: u64,
        recipe_id: &str,
        recipe_revision: u64,
        user_intent_id: &str,
        key: &str,
    ) -> Result<CustomLaunchReply, Status> {
        self.client
            .clone()
            .games_request_custom_launch(timed(Self::custom_launch_execution(
                &self.context,
                game_id,
                installation_id,
                expected_revision,
                recipe_id,
                recipe_revision,
                user_intent_id,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Removes one of this caller's recipes at the revision it last read.
    pub async fn remove_custom_launch(
        &self,
        game_id: &str,
        recipe_id: &str,
        recipe_revision: u64,
        key: &str,
    ) -> Result<MutationReply, Status> {
        self.client
            .clone()
            .games_remove_custom_launch(timed(Self::custom_launch_removal(
                &self.context,
                game_id,
                recipe_id,
                recipe_revision,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Matching (`matching.read`, `matching.write`) ----

    /// Searches the GameNode catalog by title: one page of at most `page_size` candidates (0: the
    /// host's default of 20; the host caps it at 50), in the server's order. `UNAVAILABLE` when
    /// GameNode cannot answer, never an empty list; `RESOURCE_EXHAUSTED` past the host's rate.
    pub async fn search_catalog(
        &self,
        query: &str,
        page_size: u32,
    ) -> Result<Vec<MatchCandidate>, Status> {
        self.client
            .clone()
            .matching_search(timed(MatchingSearchRequest {
                context: Some(self.context.clone()),
                query: query.to_owned(),
                platform: String::new(),
                page_size,
                cursor: String::new(),
            }))
            .await
            .map(|reply| reply.into_inner().candidates)
    }

    /// Matches an own entry to a GameNode game that the library knows or a recent
    /// [`search_catalog`](Self::search_catalog) returned. `expected_revision` is the entry's
    /// `revision`. A match the user chose is `FAILED_PRECONDITION` and is never replaced.
    pub async fn assign_match(
        &self,
        installation_id: &str,
        gamenode_game_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .matching_assign(timed(MatchingAssignRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                provider_id: String::new(),
                provider_game_id: String::new(),
                gamenode_game_id: gamenode_game_id.to_owned(),
                installation_id: installation_id.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Covers (`artwork.read`, `artwork.write`) ----

    /// The cover an own entry shows, at most `max_bytes` (0: the host's 5 MiB maximum). Nothing is
    /// downloaded: an entry whose cover is not on this device is `NOT_FOUND`, and a larger image
    /// is `RESOURCE_EXHAUSTED` rather than truncated.
    pub async fn read_cover(&self, installation_id: &str, max_bytes: u64) -> Result<Cover, Status> {
        let reply = self
            .client
            .clone()
            .artwork_read(timed(ArtworkReadRequest {
                context: Some(self.context.clone()),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                artwork_kind: String::new(),
                max_bytes,
            }))
            .await?
            .into_inner();
        Ok(Cover {
            bytes: reply.encoded_image,
            media_type: reply.media_type,
            revision: reply.revision,
        })
    }

    /// Sets the cover of an own entry from an encoded static PNG, JPEG or WebP (at most 4 MiB and
    /// 4096 x 4096 pixels; `media_type` must match the bytes). The host decodes, shrinks and
    /// re-encodes it; the matched game's cover is not touched. `expected_revision` is the entry's
    /// `artwork_revision`. A refused image is `INVALID_ARGUMENT` with the reason.
    pub async fn set_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        image: Vec<u8>,
        media_type: &str,
    ) -> Result<Written, Status> {
        self.send_cover(
            installation_id,
            expected_revision,
            key,
            image,
            media_type,
            "",
        )
        .await
    }

    /// Like [`set_cover`](Self::set_cover), but the image is a file the plugin may read:
    /// `file_handle` is a regular-file handle from the Files service (`files_list`,
    /// `files_create`), from an approval of this same installation. It needs `files.read` besides
    /// `artwork.write`; the host reads the file itself under the same 4 MiB bound, so the plugin
    /// does not have to hold the bytes. A path or URL is never accepted. `media_type` must match
    /// the file's bytes.
    pub async fn set_cover_from_file(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        file_handle: &str,
        media_type: &str,
    ) -> Result<Written, Status> {
        self.send_cover(
            installation_id,
            expected_revision,
            key,
            Vec::new(),
            media_type,
            file_handle,
        )
        .await
    }

    /// Restores the game's own cover for an own entry.
    pub async fn clear_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.send_cover(installation_id, expected_revision, key, Vec::new(), "", "")
            .await
    }

    async fn send_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        image: Vec<u8>,
        media_type: &str,
        file_handle: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .artwork_set(timed(ArtworkSetRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                artwork_kind: String::new(),
                encoded_image: image,
                media_type: media_type.to_owned(),
                file_handle: file_handle.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }
}

#[cfg(test)]
mod custom_launch_tests {
    use super::*;
    #[test]
    fn proposal_preserves_typed_custom_launch_fields() {
        let context = ServiceContext {
            provider_id: "fixture".into(),
            ..Default::default()
        };
        let recipe = CustomLaunchRecipe {
            game_id: "620".into(),
            installation_id: "steam:620".into(),
            content_id: String::new(),
            expected_revision: 12,
            executable_handle: "exec-handle".into(),
            uri: String::new(),
            argv: vec!["--literal value".into(), "$(not-a-shell)".into()],
            working_directory_handle: "cwd-handle".into(),
            environment_set: [("A".into(), "value with spaces".into())].into(),
            environment_unset: vec!["B".into()],
            runtime_handle: String::new(),
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "intent", "key");

        assert_eq!(request.context.as_ref(), Some(&context));
        assert_eq!(request.game_id, "620");
        assert_eq!(request.installation_id, "steam:620");
        assert_eq!(request.expected_revision, 12);
        assert_eq!(request.executable_handle, "exec-handle");
        assert!(request.uri.is_empty());
        assert_eq!(request.argv, recipe.argv);
        assert_eq!(request.working_directory_handle, "cwd-handle");
        assert_eq!(
            request.environment_set.get("A").map(String::as_str),
            Some("value with spaces")
        );
        assert_eq!(request.environment_unset, ["B"]);
        assert!(request.runtime_handle.is_empty());
        assert!(request.recipe_id.is_empty());
        assert_eq!(request.recipe_revision, 0);
        assert_eq!(request.user_intent_id, "intent");
        assert_eq!(request.idempotency_key, "key");
    }

    #[test]
    fn runtime_proposal_keeps_content_and_omits_direct_executable() {
        let context = ServiceContext {
            provider_id: "fixture".into(),
            ..Default::default()
        };
        let recipe = CustomLaunchRecipe {
            game_id: "620".into(),
            installation_id: "steam:620".into(),
            content_id: "content-variant-1".into(),
            expected_revision: 12,
            executable_handle: String::new(),
            uri: String::new(),
            argv: vec!["--literal value".into()],
            working_directory_handle: "cwd-handle".into(),
            environment_set: [("A".into(), "value with spaces".into())].into(),
            environment_unset: vec!["B".into()],
            runtime_handle: "runtime-handle".into(),
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "intent", "key");

        assert!(request.executable_handle.is_empty());
        assert!(request.uri.is_empty());
        assert_eq!(request.content_id, "content-variant-1");
        assert_eq!(request.runtime_handle, "runtime-handle");
        assert_eq!(request.argv, recipe.argv);
        assert_eq!(request.working_directory_handle, "cwd-handle");
        assert_eq!(
            request.environment_set.get("A").map(String::as_str),
            Some("value with spaces")
        );
        assert_eq!(request.environment_unset, ["B"]);
    }

    #[test]
    fn known_mod_layer_proposal_keeps_host_reviewed_fields_typed() {
        let context = ServiceContext::default();
        let recipe = CustomLaunchRecipe {
            game_id: "fixture-game".into(),
            installation_id: "fixture-install".into(),
            expected_revision: 7,
            executable_handle: "opaque-executable".into(),
            runtime_handle: "opaque-known-mod-layer".into(),
            argv: vec!["--profile".into(), "fixture".into()],
            ..Default::default()
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "intent", "key");

        assert_eq!(request.runtime_handle, "opaque-known-mod-layer");
        assert_eq!(request.executable_handle, "opaque-executable");
        assert_eq!(request.argv, ["--profile", "fixture"]);
        assert_eq!(request.expected_revision, 7);
    }

    #[test]
    fn environment_only_consent_stays_a_recipe_and_does_not_become_install_intent() {
        let context = ServiceContext::default();
        let recipe = CustomLaunchRecipe {
            game_id: "fixture-game".into(),
            installation_id: "fixture-install".into(),
            expected_revision: 7,
            environment_set: [("FIXTURE_MODE".into(), "enabled".into())].into(),
            environment_unset: vec!["FIXTURE_LEGACY_MODE".into()],
            ..Default::default()
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "launch-intent", "key");

        assert!(request.executable_handle.is_empty());
        assert!(request.uri.is_empty());
        assert!(request.runtime_handle.is_empty());
        assert!(request.argv.is_empty());
        assert_eq!(
            request
                .environment_set
                .get("FIXTURE_MODE")
                .map(String::as_str),
            Some("enabled")
        );
        assert_eq!(request.environment_unset, ["FIXTURE_LEGACY_MODE"]);
        assert_eq!(request.user_intent_id, "launch-intent");
        // Installation has a separate request type and intent field; launch consent cannot fill it.
        let install = GameInstallRequest {
            game_id: request.game_id.clone(),
            installation_id: request.installation_id.clone(),
            user_intent_id: "separate-install-intent".into(),
            ..Default::default()
        };
        assert_ne!(install.user_intent_id, request.user_intent_id);
    }

    #[test]
    fn uri_proposal_uses_the_uri_target_without_executable_fields() {
        let context = ServiceContext::default();
        let recipe = CustomLaunchRecipe {
            game_id: "620".into(),
            installation_id: "steam:620".into(),
            expected_revision: 12,
            uri: "game://open?id=620".into(),
            argv: vec!["--profile".into(), "alternate profile".into()],
            working_directory_handle: "cwd-handle".into(),
            environment_set: [("MODE".into(), "custom".into())].into(),
            environment_unset: vec!["LEGACY_MODE".into()],
            ..Default::default()
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "intent", "key");

        assert_eq!(request.uri, "game://open?id=620");
        assert!(request.executable_handle.is_empty());
        assert_eq!(request.argv, recipe.argv);
        assert_eq!(request.working_directory_handle, "cwd-handle");
        assert!(request.runtime_handle.is_empty());
        assert_eq!(
            request.environment_set.get("MODE").map(String::as_str),
            Some("custom")
        );
        assert_eq!(request.environment_unset, ["LEGACY_MODE"]);
    }

    #[test]
    fn execution_uses_only_the_approved_recipe_reference() {
        let context = ServiceContext::default();
        let request = Resources::custom_launch_execution(
            &context,
            "620",
            "steam:620",
            12,
            "recipe-1",
            4,
            "intent",
            "key",
        );

        assert_eq!(request.context.as_ref(), Some(&context));
        assert_eq!(request.game_id, "620");
        assert_eq!(request.installation_id, "steam:620");
        assert!(request.content_id.is_empty());
        assert_eq!(request.expected_revision, 12);
        assert_eq!(request.recipe_id, "recipe-1");
        assert_eq!(request.recipe_revision, 4);
        assert_eq!(request.user_intent_id, "intent");
        assert_eq!(request.idempotency_key, "key");
        assert!(request.executable_handle.is_empty());
        assert!(request.uri.is_empty());
        assert!(request.argv.is_empty());
        assert!(request.working_directory_handle.is_empty());
        assert!(request.environment_set.is_empty());
        assert!(request.environment_unset.is_empty());
        assert!(request.runtime_handle.is_empty());
    }

    #[test]
    fn removal_binds_recipe_revision_and_idempotency_key() {
        let context = ServiceContext {
            provider_id: "fixture".into(),
            ..Default::default()
        };
        let request = Resources::custom_launch_removal(&context, "620", "recipe-1", 4, "key");
        let mutation = request.mutation.expect("removal must be revision guarded");

        assert_eq!(request.game_id, "620");
        assert_eq!(request.recipe_id, "recipe-1");
        assert_eq!(mutation.context.as_ref(), Some(&context));
        assert_eq!(mutation.expected_revision, 4);
        assert_eq!(mutation.idempotency_key, "key");
    }
}
