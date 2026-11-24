//! The API 1.6 `ResourceHost` service. Library list/get/watch are served from the host-owned
//! [`LibraryFeed`], shared Data access through the bounded broker, and display metadata and
//! favorites through the engine's [`LibraryHost`]; other methods remain `UNIMPLEMENTED` until
//! their adapters exist, and `DescribeApi` advertises only what is served.
//!
//! Calls are plugin-initiated: they carry the session's resource context from `OpenSession`,
//! which [`ProviderApi::authorize_resource`] checks against the session, profile and grants.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::Stream;
use gamenode_platform::plugin_shared_data::{OwnerScope, RecordProvenance};
use gamenode_plugin_protocol::{
    capability as cap,
    prost_types::{self, value::Kind},
    proto::*,
};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

mod artwork;
mod backups;
mod custom_launch;
mod diagnostics;
pub(crate) mod files;
mod games;
mod jobs;
mod libraries;
mod locations;
mod matching;
mod mutations;
mod network;
mod notifications;
mod observations;
mod performance;
mod persistence;
mod ports;
mod runtimes;
mod saves;
mod storage;
mod tools;

use self::mutations::mutation_key;
use super::files::FileAccess;
use super::fingerprints;
use super::intents::Intents;
use super::jobs::Jobs;
use super::library::{
    Change, CursorBinding, CursorError, DEFAULT_PAGE_SIZE, LibraryFeed, MAX_PAGE_SIZE,
    MAX_SEARCH_BYTES, ResumeError, Scope,
};
use super::network::NetworkAccess;
use super::notifications::Notifications;
use super::provider::{ProviderApi, ResourceCaller};
use super::saves::SaveAccess;
use super::shared_data::{
    BrokerError, DataBroker, ProjectedRecord, QueryCursor, QueryRequest, SubscriptionEnd,
};
use super::shared_pool::{DurableSharedPool, DurableWrite};
use super::sharing_policy::{AccessDecision, AccessRequest, Authorizer, Delivery};
use super::storage::PluginStorage;

/// Open `LibraryWatch` streams allowed per plugin session.
pub const MAX_WATCHES_PER_SESSION: usize = 8;
/// How often an idle watch rechecks its authorization.
const WATCH_RECHECK: Duration = Duration::from_secs(5);
const WATCH_QUEUE: usize = 16;
const MAX_PROVIDER_ID_BYTES: usize = 128;
const MAX_DATA_CURSORS: usize = 4096;
const DATA_CURSOR_TTL: Duration = Duration::from_secs(5 * 60);

/// The ResourceHost methods this build serves, for `DescribeApi`.
pub const SERVED: &[Served] = &[
    Served::new("PortsList", cap::PORTS_READ, true, "host catalog"),
    Served::new("PortsDetail", cap::PORTS_READ, false, "host catalog"),
    Served::new(
        "PortsRequestInstall",
        cap::PORTS_INSTALL,
        false,
        "host catalog; Core guided setup",
    ),
    Served::new(
        "ToolsList",
        cap::TOOLS_READ,
        true,
        "host catalog; scoped game",
    ),
    Served::new(
        "ToolsRequestApply",
        cap::TOOLS_APPLY,
        false,
        "scoped game; Core tools review",
    ),
    Served::new(
        "ToolsRequestRemove",
        cap::TOOLS_REMOVE,
        false,
        "scoped game; Core tools review",
    ),
    Served::new(
        "BackupsList",
        cap::BACKUPS_READ,
        true,
        "approved save profile",
    ),
    Served::new(
        "BackupsRequestCreate",
        cap::BACKUPS_CREATE,
        false,
        "approved save profile",
    ),
    Served::new(
        "BackupsRequestRestore",
        cap::BACKUPS_RESTORE,
        false,
        "approved save profile; confirmation required",
    ),
    Served::new(
        "StorageDiagnosticsPublishBenchmark",
        cap::STORAGE_DIAGNOSTICS_PUBLISH,
        false,
        "host-attributed volume report; Core starts no benchmark process",
    ),
    Served::new(
        "StorageDiagnosticsPublishSmart",
        cap::STORAGE_DIAGNOSTICS_PUBLISH,
        false,
        "host-attributed volume report; Core starts no SMART process",
    ),
    Served::new(
        "ComputerPublishHardware",
        cap::COMPUTER_HARDWARE_PUBLISH,
        false,
        "host-attributed sensitive hardware observation; no collector is started",
    ),
    Served::new(
        "GamePresencePublish",
        cap::GAME_PRESENCE_PUBLISH,
        false,
        "host-attributed sensitive game status with bounded expiry",
    ),
    Served::new(
        "GamePresenceClear",
        cap::GAME_PRESENCE_PUBLISH,
        false,
        "the caller's own game/session presence",
    ),
    Served::new(
        "GamePerformancePublishSamples",
        cap::GAME_PERFORMANCE_PUBLISH,
        false,
        "authenticated, bounded session-retained performance series",
    ),
    Served::new(
        "GamePerformanceEndSeries",
        cap::GAME_PERFORMANCE_PUBLISH,
        false,
        "the caller's own performance series",
    ),
    Served::new(
        "LibrariesList",
        cap::LIBRARY_ROOTS_READ,
        true,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesGet",
        cap::LIBRARY_ROOTS_READ,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesWatch",
        cap::LIBRARY_ROOTS_READ,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesCreate",
        cap::LIBRARY_ROOTS_CREATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesUpdate",
        cap::LIBRARY_ROOTS_UPDATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesRemove",
        cap::LIBRARY_ROOTS_DELETE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesAddRoot",
        cap::LIBRARY_ROOTS_CREATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesUpdateRoot",
        cap::LIBRARY_ROOTS_UPDATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "LibrariesRemoveRoot",
        cap::LIBRARY_ROOTS_DELETE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new("LibraryList", cap::LIBRARY_READ, true, READ_SCOPE),
    Served::new("LibraryGet", cap::LIBRARY_READ, false, READ_SCOPE),
    Served::new("LibraryWatch", cap::LIBRARY_READ, false, READ_SCOPE),
    Served::new(
        "RuntimesList",
        cap::GAME_RUNTIME_READ,
        true,
        RUNTIME_READ_SCOPE,
    ),
    Served::new(
        "RuntimesGet",
        cap::GAME_RUNTIME_READ,
        false,
        RUNTIME_READ_SCOPE,
    ),
    Served::new(
        "RuntimesWatch",
        cap::GAME_RUNTIME_READ,
        false,
        RUNTIME_READ_SCOPE,
    ),
    Served::new(
        "RuntimesRequestAccess",
        cap::GAME_RUNTIME_CREATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "RuntimesRegister",
        cap::GAME_RUNTIME_CREATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "RuntimesUpdate",
        cap::GAME_RUNTIME_UPDATE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "RuntimesRemove",
        cap::GAME_RUNTIME_DELETE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new("GamesRegisterContent", cap::DISCOVERY, false, OWN_OR_TARGET),
    Served::new(
        "MetadataUpdate",
        cap::LIBRARY_METADATA_WRITE,
        false,
        OWN_ENTRIES,
    ),
    Served::new("FavoritesList", cap::FAVORITES_READ, true, OWN_ENTRIES),
    Served::new("FavoritesSet", cap::FAVORITES_WRITE, false, OWN_ENTRIES),
    Served::new("MatchingSearch", cap::MATCHING_READ, false, CATALOG),
    Served::new("MatchingAssign", cap::MATCHING_WRITE, false, OWN_ENTRIES),
    Served::new("ArtworkRead", cap::ARTWORK_READ, false, OWN_ENTRIES),
    Served::new("ArtworkSet", cap::ARTWORK_WRITE, false, OWN_ENTRIES),
    Served::new("StorageGet", cap::STORAGE_READ, false, PRIVATE_NAMESPACE),
    Served::new("StoragePut", cap::STORAGE_WRITE, false, PRIVATE_NAMESPACE),
    Served::new(
        "StorageDelete",
        cap::STORAGE_DELETE,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "PersistenceGetUsage",
        cap::STORAGE_PERSIST,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "PersistenceListEntries",
        cap::STORAGE_PERSIST,
        true,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "PersistenceDeleteEntries",
        cap::STORAGE_PERSIST,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "PersistenceClear",
        cap::STORAGE_PERSIST,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "PersistenceSetRetentionPolicy",
        cap::STORAGE_PERSIST,
        false,
        PRIVATE_NAMESPACE,
    ),
    Served::new(
        "FilesRequestAccess",
        cap::FILES_READ,
        false,
        APPROVED_FOLDERS,
    ),
    Served::new("FilesList", cap::FILES_READ, true, APPROVED_FOLDERS),
    Served::new("FilesRead", cap::FILES_READ, false, APPROVED_FOLDERS),
    Served::new("FilesWrite", cap::FILES_WRITE, false, APPROVED_FOLDERS),
    Served::new("FilesDelete", cap::FILES_DELETE, false, APPROVED_FOLDERS),
    Served::new("NetworkFetch", cap::NETWORK_FETCH, false, APPROVED_ORIGINS),
    Served::new(
        "NotificationsPublish",
        cap::NOTIFICATIONS_PUBLISH,
        false,
        OWN_NOTICES,
    ),
    Served::new("GamesRequestLaunch", cap::GAME_LAUNCH, false, OWN_OR_TARGET),
    Served::new(
        "GamesProposeCustomLaunch",
        cap::GAME_CUSTOM_LAUNCH,
        false,
        OWN_OR_TARGET,
    ),
    Served::new(
        "GamesRequestCustomLaunch",
        cap::GAME_CUSTOM_LAUNCH,
        false,
        OWN_OR_TARGET,
    ),
    Served::new(
        "GamesRemoveCustomLaunch",
        cap::GAME_CUSTOM_LAUNCH,
        false,
        OWN_OR_TARGET,
    ),
    Served::new(
        "GamesRequestInstall",
        cap::GAMES_INSTALL,
        false,
        OWN_OR_TARGET,
    ),
    Served::new("JobsGet", UNDERLYING, false, OWN_JOBS),
    Served::new("JobsWatch", UNDERLYING, false, OWN_JOBS),
    Served::new("JobsCancel", UNDERLYING, false, OWN_JOBS),
    Served::new("DataPublish", cap::DATA_PUBLISH, false, DATA_SCOPE),
    Served::new("DataGet", DATA_READ_SCOPE, false, DATA_SCOPE),
    Served::new("DataList", DATA_READ_SCOPE, true, DATA_SCOPE),
    Served::new("DataWatch", DATA_READ_SCOPE, false, DATA_SCOPE),
];

const OWN_OR_TARGET: &str =
    "own provider entries; an extension session's selected game; needs the user's intent";

/// Jobs have no permission of their own: each needs the one of the operation that made it.
const UNDERLYING: &str = "";
const OWN_JOBS: &str = "jobs this plugin started in this profile; the permission of the operation that started the job";

const READ_SCOPE: &str = "own provider; library.read.all for every provider";
const OWN_ENTRIES: &str = "own provider entries";
const CATALOG: &str = "GameNode catalog (title search)";
const PRIVATE_NAMESPACE: &str = "own plugin/publisher/profile namespace";
/// Runtime reads stay owner-private unless the query explicitly selects the common catalog.
const RUNTIME_READ_SCOPE: &str =
    "own runtimes; granted runtime catalog category for other plugins' shared records";
const OWN_NOTICES: &str = "plain text attributed to the calling plugin";
const APPROVED_ORIGINS: &str = "exact HTTPS origins the user approved for this plugin";
const APPROVED_FOLDERS: &str = "folder the user approved for this plugin and installation";
const DATA_READ_SCOPE: &str = "authenticated owner data or explicit current sharing grants";
const DATA_SCOPE: &str = "host-attributed dataset fields projected under current sharing policy";

/// The `Saves` service methods this build serves, for `DescribeApi`.
pub const SAVES_SERVED: &[Served] = saves::SERVED;

/// The `Locations` service methods this build serves, for `DescribeApi`.
pub const LOCATIONS_SERVED: &[Served] = locations::SERVED;

/// The `StorageDiagnostics` service methods this build serves, for `DescribeApi`.
pub const DIAGNOSTICS_SERVED: &[Served] = diagnostics::SERVED;

/// The `Computer` service methods this build serves, for `DescribeApi`.
pub const COMPUTER_SERVED: &[Served] = observations::COMPUTER_SERVED;

/// The `GamePresence` service methods this build serves, for `DescribeApi`.
pub const PRESENCE_SERVED: &[Served] = observations::PRESENCE_SERVED;

/// The `GamePerformance` service methods this build serves, for `DescribeApi`.
pub const PERFORMANCE_SERVED: &[Served] = performance::SERVED;

/// A served method's `DescribeApi` facts.
pub struct Served {
    pub method: &'static str,
    pub permission: &'static str,
    pub paged: bool,
    pub ownership: &'static str,
}

impl Served {
    const fn new(
        method: &'static str,
        permission: &'static str,
        paged: bool,
        ownership: &'static str,
    ) -> Self {
        Self {
            method,
            permission,
            paged,
            ownership,
        }
    }
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub struct ResourceApi {
    providers: ProviderApi,
    library: Arc<LibraryFeed>,
    watches: Arc<Mutex<HashMap<String, usize>>>,
    mutations: Arc<mutations::Mutations>,
    searches: Arc<matching::SearchLimiter>,
    /// Bounds the plugin pictures being decoded at once.
    decoders: Arc<tokio::sync::Semaphore>,
    /// Bounds backup/history workers, including calls that are not jobs.
    backup_slots: Arc<tokio::sync::Semaphore>,
    /// The plugins' private key/value storage.
    storage: Arc<PluginStorage>,
    /// File approvals and handles.
    files: Arc<FileAccess>,
    /// Approved origins and limits for network fetches.
    network: Arc<NetworkAccess>,
    /// The notifications plugins ask Core to show.
    notifications: Arc<Notifications>,
    /// The long operations plugins started.
    jobs: Jobs,
    /// What the user wanted to start.
    intents: Arc<Intents>,
    /// The grants for other plugins' save profiles.
    saves: Arc<SaveAccess>,
    /// The content fingerprints of approved places.
    snapshots: Arc<fingerprints::Snapshots>,
    libraries: Arc<libraries::Libraries>,
    /// Host-owned records and policy projection boundary for domain/Data reads.
    data: Arc<DataBroker>,
    /// The durable public pool; refuses durable publication when the host has no data folder.
    shared_pool: Arc<DurableSharedPool>,
    shared_authorizer: Arc<dyn Authorizer>,
    data_cursors: Arc<Mutex<HashMap<String, DataCursorEntry>>>,
    performance_batch_times: Arc<Mutex<HashMap<String, Instant>>>,
    performance_cursors: Arc<Mutex<HashMap<String, performance::CursorEntry>>>,
    /// Host-owned coordinator shared with supervised launches, when connected by the application.
    leases: Arc<crate::plugin_host::resource_leases::ResourceLeases>,
    /// Platform process inspection; absent in tests or unsupported builds, where checks fail
    /// closed.
    processes: Option<Arc<dyn gamenode_core::ports::Processes>>,
}

struct DataCursorEntry {
    session_id: String,
    cursor: QueryCursor,
    scope_hash: Option<[u8; 32]>,
    expires_at: Instant,
}

/// Unit-test and compatibility fallback: own records remain available after the endpoint's
/// session check, while foreign records are denied unless production injects the sharing store.
struct DenySharedData;

impl Authorizer for DenySharedData {
    fn revision(&self, _recipient: &super::grants::PluginIdentity) -> u64 {
        0
    }

    fn decide(&self, _request: &AccessRequest<'_>) -> AccessDecision {
        AccessDecision::default()
    }
}

impl ResourceApi {
    pub(crate) async fn confirm_runtime_context(
        &self,
        plugin_id: String,
        publisher_fingerprint: String,
        profile_epoch: u64,
        game_id: String,
        installation_id: String,
        game_name: String,
    ) -> Result<bool, Status> {
        use super::intents::{Binding, ConfirmationRequest, Operation, Verdict};
        let _pending = self.intents.begin_pending(&Binding {
            plugin_id: plugin_id.clone(),
            publisher_fingerprint: publisher_fingerprint.clone(),
            profile_epoch,
            game_id,
            installation_id,
            operation: Operation::RuntimeSessionContext,
            target_revision: 0,
            recipe_id: String::new(),
            recipe_revision: 0,
            plan_fingerprint: String::new(),
        })?;
        Ok(matches!(
            self.intents
                .confirm(ConfirmationRequest {
                    plugin_id,
                    publisher_fingerprint,
                    game_name,
                    operation: Operation::RuntimeSessionContext,
                    details: Vec::new(),
                    launch_plan: None,
                })
                .await?,
            Verdict::Approved { .. }
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        providers: ProviderApi,
        library: Arc<LibraryFeed>,
        storage: Arc<PluginStorage>,
        files: Arc<FileAccess>,
        network: Arc<NetworkAccess>,
        notifications: Arc<Notifications>,
        jobs: Jobs,
        intents: Arc<Intents>,
        saves: Arc<SaveAccess>,
        snapshots: Arc<fingerprints::Snapshots>,
        leases: Arc<crate::plugin_host::resource_leases::ResourceLeases>,
        processes: Option<Arc<dyn gamenode_core::ports::Processes>>,
    ) -> Self {
        Self {
            providers,
            libraries: Arc::default(),
            jobs,
            intents,
            saves,
            snapshots,
            library,
            storage,
            files,
            network,
            notifications,
            watches: Arc::default(),
            mutations: Arc::default(),
            searches: Arc::default(),
            decoders: Arc::new(tokio::sync::Semaphore::new(artwork::DECODE_SLOTS)),
            backup_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            data: Arc::new(DataBroker::new()),
            shared_pool: Arc::new(DurableSharedPool::disabled()),
            shared_authorizer: Arc::new(DenySharedData),
            data_cursors: Arc::default(),
            performance_batch_times: Arc::default(),
            performance_cursors: Arc::default(),
            leases,
            processes,
        }
    }

    /// Attach the host's live sharing policy index. Production owns its persistence and passes
    /// the same revisioned policy used by settings/activation reviews.
    pub fn with_shared_data(
        mut self,
        data: Arc<DataBroker>,
        authorizer: Arc<dyn Authorizer>,
    ) -> Self {
        self.providers.set_shared_data_broker(&data);
        self.data = data;
        self.shared_authorizer = authorizer;
        self
    }

    /// Attach the host-owned durable public pool. Production supplies the application data
    /// folder; without one a durable publication fails instead of falling back to the session.
    pub fn with_shared_pool(mut self, pool: Arc<DurableSharedPool>) -> Self {
        self.shared_pool = pool;
        self
    }

    fn scope(caller: &ResourceCaller, provider_id: &str) -> Result<Scope, Status> {
        if provider_id.len() > MAX_PROVIDER_ID_BYTES {
            return Err(Status::invalid_argument("provider_id is too long"));
        }
        Ok(Scope {
            own_provider: caller.provider_id.clone(),
            read_all: caller.has(cap::LIBRARY_READ_ALL),
            provider_filter: (!provider_id.is_empty()).then(|| provider_id.to_owned()),
        })
    }

    /// Holds one of the session's watch slots until dropped.
    fn watch_slot(&self, session_id: &str) -> Result<WatchSlot, Status> {
        let mut watches = self.watches.lock().unwrap_or_else(|p| p.into_inner());
        let open = watches.entry(session_id.to_owned()).or_default();
        if *open >= MAX_WATCHES_PER_SESSION {
            return Err(Status::resource_exhausted(
                "too many open watches for this session",
            ));
        }
        *open += 1;
        Ok(WatchSlot {
            watches: self.watches.clone(),
            session_id: session_id.to_owned(),
        })
    }

    fn data_identity(caller: &ResourceCaller) -> Result<super::grants::PluginIdentity, Status> {
        caller
            .namespace
            .as_ref()
            .map(|namespace| namespace.identity().clone())
            .ok_or_else(|| {
                Status::permission_denied(
                    "shared plugin data is available only to authenticated third-party plugins",
                )
            })
    }

    fn take_data_cursor(
        &self,
        token: &str,
        session_id: &str,
    ) -> Result<Option<QueryCursor>, Status> {
        self.take_scoped_data_cursor(token, session_id, None)
    }

    fn take_scoped_data_cursor(
        &self,
        token: &str,
        session_id: &str,
        scope_hash: Option<[u8; 32]>,
    ) -> Result<Option<QueryCursor>, Status> {
        if token.is_empty() {
            return Ok(None);
        }
        if token.len() != 48 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Status::invalid_argument("invalid data cursor"));
        }
        let mut cursors = self
            .data_cursors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cursors.retain(|_, entry| entry.expires_at > Instant::now());
        let Some(entry) = cursors.get(token) else {
            return Err(Status::failed_precondition(
                "data cursor is stale; list again",
            ));
        };
        if entry.session_id != session_id {
            return Err(Status::invalid_argument("invalid data cursor"));
        }
        if entry.scope_hash != scope_hash {
            return Err(Status::invalid_argument(
                "data cursor belongs to another query",
            ));
        }
        Ok(Some(entry.cursor.clone()))
    }

    fn issue_data_cursor(&self, session_id: &str, cursor: QueryCursor) -> Result<String, Status> {
        self.issue_scoped_data_cursor(session_id, cursor, None)
    }

    fn issue_scoped_data_cursor(
        &self,
        session_id: &str,
        cursor: QueryCursor,
        scope_hash: Option<[u8; 32]>,
    ) -> Result<String, Status> {
        let mut cursors = self
            .data_cursors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cursors.retain(|_, entry| entry.expires_at > Instant::now());
        if cursors.len() >= MAX_DATA_CURSORS
            && let Some(oldest) = cursors
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(token, _)| token.clone())
        {
            cursors.remove(&oldest);
        }
        let mut bytes = [0_u8; 24];
        getrandom::fill(&mut bytes)
            .map_err(|_| Status::internal("no source of randomness for data cursor"))?;
        let token = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        cursors.insert(
            token.clone(),
            DataCursorEntry {
                session_id: session_id.to_owned(),
                cursor,
                scope_hash,
                expires_at: Instant::now() + DATA_CURSOR_TTL,
            },
        );
        Ok(token)
    }
}

struct WatchSlot {
    watches: Arc<Mutex<HashMap<String, usize>>>,
    session_id: String,
}

impl Drop for WatchSlot {
    fn drop(&mut self) {
        let mut watches = self.watches.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(open) = watches.get_mut(&self.session_id) {
            *open -= 1;
            if *open == 0 {
                watches.remove(&self.session_id);
            }
        }
    }
}

/// Library projections for common-data categories (such as runtime availability) are not part
/// of this build; a query that selects one is refused rather than silently narrowed.
fn no_common_data(selector: Option<&CommonDataSelector>) -> Result<(), Status> {
    match selector {
        Some(selector)
            if !selector.categories.is_empty() || !selector.selected_record_ids.is_empty() =>
        {
            Err(Status::unimplemented(
                "common-data projections are not available for Library",
            ))
        }
        _ => Ok(()),
    }
}

/// Projection names a common content-availability reader may see. No path, handle, byte payload
/// or private owner key is part of it.
const LIBRARY_AVAILABILITY_FIELDS: &[&str] = &[
    "content_id",
    "content_kind",
    "runtime_id",
    "runtime_name",
    "platform",
    "publisher_id",
    "revision",
];

impl ResourceApi {
    /// Returns whether this Library query explicitly selected the content-availability category
    /// and is currently granted. An empty selector keeps the query owner-private.
    fn shared_content_availability(
        &self,
        caller: &ResourceCaller,
        selector: Option<&CommonDataSelector>,
    ) -> Result<bool, Status> {
        let Some(selector) = selector else {
            return Ok(false);
        };
        if !selector.selected_record_ids.is_empty() {
            return Err(Status::unimplemented(
                "selected-record content availability is not available in this build",
            ));
        }
        if selector.categories.is_empty() {
            return Ok(false);
        }
        if selector.categories != [CommonDataCategory::ContentAvailability as i32] {
            return Err(Status::unimplemented(
                "common-data projections for Library support only content availability",
            ));
        }
        let recipient = caller
            .namespace
            .as_ref()
            .map(|namespace| namespace.identity().clone())
            .ok_or_else(|| {
                Status::failed_precondition(
                    "a third-party plugin identity is required for shared catalog reads",
                )
            })?;
        let fields = LIBRARY_AVAILABILITY_FIELDS
            .iter()
            .map(|field| (*field).to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        if !self
            .shared_authorizer
            .common_category(&recipient, CommonDataCategory::ContentAvailability, &fields)
            .allowed
        {
            return Err(Status::permission_denied(
                "common content-availability access is not granted",
            ));
        }
        Ok(true)
    }

    /// Loads the safe availability projection for the requested installations and groups it by
    /// installation id. `include_shared` requires a previously authorized category query.
    async fn runtime_availability_map(
        &self,
        caller: &ResourceCaller,
        include_shared: bool,
        installation_ids: Vec<String>,
    ) -> Result<HashMap<String, Vec<RuntimeAvailability>>, Status> {
        if installation_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let Some(host) = self.library.host() else {
            // Private reads keep working without a connected host; availability is additive.
            if include_shared {
                return Err(mutations::unavailable());
            }
            return Ok(HashMap::new());
        };
        let owner = if include_shared {
            None
        } else {
            Some(mutations::mutation_owner(caller))
        };
        let rows = host.runtime_availability(owner, installation_ids).await?;
        let mut grouped: HashMap<String, Vec<RuntimeAvailability>> = HashMap::new();
        for row in rows {
            grouped
                .entry(row.installation_id)
                .or_default()
                .push(RuntimeAvailability {
                    content_id: row.content_id,
                    content_kind: row.content_kind,
                    runtime_id: row.runtime_id,
                    runtime_name: row.runtime_name,
                    platform: row.platform,
                    publisher_id: row.publisher_id,
                    revision: row.revision,
                });
        }
        Ok(grouped)
    }

    async fn library_event(
        &self,
        caller: &ResourceCaller,
        include_shared: bool,
        change: &Change,
    ) -> Result<LibraryEvent, Status> {
        Ok(LibraryEvent {
            revision: change.revision,
            event_type: change.kind.as_str().to_owned(),
            entry: Some(
                self.entry_with_availability(caller, include_shared, change.entry.to_proto())
                    .await?,
            ),
        })
    }

    async fn entry_with_availability(
        &self,
        caller: &ResourceCaller,
        include_shared: bool,
        mut entry: LibraryEntry,
    ) -> Result<LibraryEntry, Status> {
        let installation_id = entry.installation_id.clone();
        let mut map = self
            .runtime_availability_map(caller, include_shared, vec![installation_id.clone()])
            .await?;
        entry.runtime_availability = map.remove(&installation_id).unwrap_or_default();
        Ok(entry)
    }
}

fn stale_cursor() -> Status {
    Status::failed_precondition("library cursor is stale; restart the list")
}

const MAX_DATA_VALUE_DEPTH: usize = 32;
const MAX_DATA_VALUE_ITEMS: usize = 4096;

fn struct_to_json(value: &prost_types::Struct) -> Result<JsonMap<String, JsonValue>, Status> {
    fn convert_struct(
        value: &prost_types::Struct,
        depth: usize,
        items: &mut usize,
    ) -> Result<JsonMap<String, JsonValue>, Status> {
        if depth > MAX_DATA_VALUE_DEPTH {
            return Err(Status::invalid_argument("data value is nested too deeply"));
        }
        let mut result = JsonMap::new();
        for (key, value) in &value.fields {
            *items += 1;
            if *items > MAX_DATA_VALUE_ITEMS
                || key.is_empty()
                || key.len() > 256
                || key.chars().any(char::is_control)
            {
                return Err(Status::invalid_argument(
                    "data value has invalid or excessive fields",
                ));
            }
            result.insert(key.clone(), convert_value(value, depth + 1, items)?);
        }
        Ok(result)
    }

    fn convert_value(
        value: &prost_types::Value,
        depth: usize,
        items: &mut usize,
    ) -> Result<JsonValue, Status> {
        let kind = value
            .kind
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("data value contains an unset value"))?;
        match kind {
            Kind::NullValue(_) => Ok(JsonValue::Null),
            Kind::NumberValue(number) => JsonNumber::from_f64(*number)
                .map(JsonValue::Number)
                .ok_or_else(|| Status::invalid_argument("data value contains a non-finite number")),
            Kind::StringValue(value) => Ok(JsonValue::String(value.clone())),
            Kind::BoolValue(value) => Ok(JsonValue::Bool(*value)),
            Kind::StructValue(value) => Ok(JsonValue::Object(convert_struct(value, depth, items)?)),
            Kind::ListValue(value) => {
                let mut result = Vec::with_capacity(value.values.len());
                for item in &value.values {
                    *items += 1;
                    if *items > MAX_DATA_VALUE_ITEMS {
                        return Err(Status::invalid_argument("data value has too many items"));
                    }
                    result.push(convert_value(item, depth + 1, items)?);
                }
                Ok(JsonValue::Array(result))
            }
        }
    }

    let mut items = 0;
    convert_struct(value, 1, &mut items)
}

fn json_to_struct(value: &JsonMap<String, JsonValue>) -> prost_types::Struct {
    fn convert(value: &JsonValue) -> prost_types::Value {
        let kind = match value {
            JsonValue::Null => Kind::NullValue(0),
            JsonValue::Bool(value) => Kind::BoolValue(*value),
            JsonValue::Number(value) => Kind::NumberValue(value.as_f64().unwrap_or_default()),
            JsonValue::String(value) => Kind::StringValue(value.clone()),
            JsonValue::Array(values) => Kind::ListValue(prost_types::ListValue {
                values: values.iter().map(convert).collect(),
            }),
            JsonValue::Object(values) => Kind::StructValue(json_to_struct(values)),
        };
        prost_types::Value { kind: Some(kind) }
    }

    prost_types::Struct {
        fields: value
            .iter()
            .map(|(key, value)| (key.clone(), convert(value)))
            .collect(),
    }
}

fn data_record(record: &ProjectedRecord) -> DataRecord {
    DataRecord {
        record_id: record.record_id().to_owned(),
        dataset_id: record.dataset_id().to_owned(),
        publisher_id: record.provenance().source.plugin_id.clone(),
        revision: record.revision(),
        value: (!record.fields().is_empty()).then(|| json_to_struct(record.fields())),
        schema_id: record.schema().0.to_owned(),
        schema_version: record.schema().1,
        opaque_payload: record.opaque_payload().to_vec(),
        provenance: Some(DataProvenance {
            producer_plugin_id: record.provenance().source.plugin_id.clone(),
            declared_developer: record.provenance().declared_developer.clone(),
            origin: record.provenance().origin.clone(),
            publisher_fingerprint: record.provenance().source.publisher_fingerprint.clone(),
            package_version: record.provenance().package_version.clone(),
            package_digest: record.provenance().package_digest.clone(),
            resource_id: record.resource_id().to_owned(),
            observed_at: record.observed_at(),
            received_at: record.received_at(),
            source_record_ids: record.provenance().source_record_ids.clone(),
        }),
        resource_id: record.resource_id().to_owned(),
        observed_at: record.observed_at(),
        received_at: record.received_at(),
        expires_at: record.expires_at().unwrap_or_default(),
        sequence: record.sequence(),
        source_observation_id: record.source_observation_id().to_owned(),
        durability: if record.is_durable() {
            StorageDurability::Persistent as i32
        } else {
            StorageDurability::SessionOnly as i32
        },
    }
}

fn unix_now() -> Result<i64, Status> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::internal("system clock is before the Unix epoch"))?
        .as_secs()
        .try_into()
        .map_err(|_| Status::internal("system clock exceeds the supported range"))
}

fn data_broker_status(error: BrokerError) -> Status {
    match &error {
        BrokerError::InvalidSelector
        | BrokerError::InvalidPublication
        | BrokerError::ReservedField => Status::invalid_argument(error.to_string()),
        BrokerError::RecordTooLarge | BrokerError::QuotaExceeded => {
            Status::resource_exhausted(error.to_string())
        }
        BrokerError::StaleCursor
        | BrokerError::AuthorizationChanged
        | BrokerError::StaleSubscription
        | BrokerError::SequenceConflict(_) => Status::failed_precondition(error.to_string()),
        BrokerError::WrongRecipient | BrokerError::Expired => {
            Status::permission_denied(error.to_string())
        }
    }
}

#[tonic::async_trait]
impl resource_host_server::ResourceHost for ResourceApi {
    type LibraryWatchStream = BoxStream<LibraryEvent>;
    type LibrariesWatchStream = BoxStream<LibraryRecordEvent>;
    type RuntimesWatchStream = BoxStream<RuntimeRecordEvent>;
    type DataWatchStream = BoxStream<DataEvent>;
    type JobsWatchStream = BoxStream<JobEvent>;

    async fn library_list(
        &self,
        request: Request<LibraryListRequest>,
    ) -> Result<Response<LibraryListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::LIBRARY_READ)?;
        let include_shared =
            self.shared_content_availability(&caller, page.common_data_categories.as_ref())?;
        let scope = Self::scope(&caller, &request.provider_id)?;
        let search = request.search.trim();
        if search.len() > MAX_SEARCH_BYTES {
            return Err(Status::invalid_argument("search is too long"));
        }
        let search = (!search.is_empty()).then(|| search.to_owned());
        let limit = match page.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let binding = CursorBinding {
            route: "library",
            session_id: caller.session_id.clone(),
            profile_epoch: caller.profile_epoch,
            grant_revision: caller.grant_revision,
            scope,
            search,
        };
        let resumed =
            if page.cursor.is_empty() {
                None
            } else {
                Some(self.library.open_cursor(&page.cursor, &binding).map_err(
                    |error| match error {
                        CursorError::Malformed => {
                            Status::invalid_argument("invalid library cursor")
                        }
                        CursorError::Stale => stale_cursor(),
                    },
                )?)
            };
        let (revision, entries, more) = self.library.page(
            &binding.scope,
            binding.search.as_deref(),
            resumed.as_ref().map(|(after, _)| after.as_str()),
            limit,
        );
        // The page must come from the revision the cursor was issued for.
        if resumed
            .as_ref()
            .is_some_and(|(_, issued)| *issued != revision)
        {
            return Err(stale_cursor());
        }
        let next_cursor = match entries.last() {
            Some(last) if more => {
                self.library
                    .issue_cursor(&binding, revision, &last.installation_id)
            }
            _ => String::new(),
        };
        let installation_ids = entries
            .iter()
            .map(|entry| entry.installation_id.clone())
            .collect::<Vec<_>>();
        let mut availability = self
            .runtime_availability_map(&caller, include_shared, installation_ids)
            .await?;
        let mut projected = Vec::with_capacity(entries.len());
        for entry in &entries {
            let mut proto = entry.to_proto();
            proto.runtime_availability = availability
                .remove(&entry.installation_id)
                .unwrap_or_default();
            projected.push(proto);
        }
        Ok(Response::new(LibraryListReply {
            entries: projected,
            next_cursor,
            revision,
        }))
    }

    async fn library_get(
        &self,
        request: Request<LibraryGetRequest>,
    ) -> Result<Response<LibraryGetReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::LIBRARY_READ)?;
        let include_shared =
            self.shared_content_availability(&caller, request.common_data_categories.as_ref())?;
        let scope = Self::scope(&caller, "")?;
        let game_id = match request.game_id.as_str() {
            "" => None,
            text => Some(
                text.parse::<u64>()
                    .map_err(|_| Status::invalid_argument("invalid game_id"))?,
            ),
        };
        let not_found = || Status::not_found("library entry not found");
        let (revision, entry) = if !request.installation_id.is_empty() {
            let (revision, entry) = self.library.get(&scope, &request.installation_id);
            // Naming both requires them to agree.
            let entry = entry.filter(|entry| game_id.is_none() || entry.game_id == game_id);
            (revision, entry.ok_or_else(not_found)?)
        } else if let Some(game_id) = game_id {
            let (revision, mut entries) = self.library.by_game(&scope, game_id);
            match entries.len() {
                0 => return Err(not_found()),
                1 => (revision, entries.remove(0)),
                _ => {
                    return Err(Status::failed_precondition(
                        "several installations match this game; name an installation_id",
                    ));
                }
            }
        } else {
            return Err(Status::invalid_argument(
                "installation_id or game_id is required",
            ));
        };
        let entry = self
            .entry_with_availability(&caller, include_shared, entry.to_proto())
            .await?;
        Ok(Response::new(LibraryGetReply {
            entry: Some(entry),
            revision,
        }))
    }

    async fn library_watch(
        &self,
        request: Request<LibraryWatchRequest>,
    ) -> Result<Response<Self::LibraryWatchStream>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let caller = self
            .providers
            .authorize_resource(context.as_ref(), cap::LIBRARY_READ)?;
        let include_shared =
            self.shared_content_availability(&caller, request.common_data_categories.as_ref())?;
        Self::scope(&caller, &request.provider_id)?;
        let slot = self.watch_slot(&caller.session_id)?;
        let (_, backlog, mut live) = self.library.subscribe(request.after_revision).map_err(
            |error| match error {
                ResumeError::Resync => Status::failed_precondition(
                    "watch position is no longer retained; list again and watch from its revision",
                ),
                ResumeError::Future => Status::out_of_range("watch position is in the future"),
            },
        )?;
        let (tx, rx) = mpsc::channel(WATCH_QUEUE);
        let providers = self.providers.clone();
        let resource = self.clone();
        let provider_filter = request.provider_id;
        let selector = request.common_data_categories;
        tokio::spawn(async move {
            let _slot = slot;
            // Every delivery rechecks the session, profile and grants; a grant change ends the
            // stream so the client lists again under its new scope.
            let check = || -> Result<Scope, Status> {
                let current = providers.authorize_resource(context.as_ref(), cap::LIBRARY_READ)?;
                if current.grant_revision != caller.grant_revision {
                    return Err(Status::failed_precondition(
                        "plugin permissions changed; list again",
                    ));
                }
                if resource.shared_content_availability(&current, selector.as_ref())?
                    != include_shared
                {
                    return Err(Status::failed_precondition(
                        "common-data permissions changed; list again",
                    ));
                }
                Self::scope(&current, &provider_filter)
            };
            for change in &backlog {
                let scope = match check() {
                    Ok(scope) => scope,
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        return;
                    }
                };
                if !scope.admits(&change.entry) {
                    continue;
                }
                let item = resource
                    .library_event(&caller, include_shared, change)
                    .await;
                if tx.send(item).await.is_err() {
                    return;
                }
            }
            let mut recheck = tokio::time::interval(WATCH_RECHECK);
            recheck.tick().await;
            loop {
                let change = tokio::select! {
                    received = live.recv() => match received {
                        Ok(change) => Some(change),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let _ = tx
                                .send(Err(Status::failed_precondition(
                                    "library watch fell behind; list again",
                                )))
                                .await;
                            return;
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    },
                    _ = recheck.tick() => None,
                    () = tx.closed() => return,
                };
                let scope = match check() {
                    Ok(scope) => scope,
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        return;
                    }
                };
                if let Some(change) = change.as_ref()
                    && scope.admits(&change.entry)
                {
                    let item = resource
                        .library_event(&caller, include_shared, change)
                        .await;
                    if tx.send(item).await.is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn data_publish(
        &self,
        request: Request<DataPublishRequest>,
    ) -> Result<Response<DataMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let (caller, _session_commit) = self
            .providers
            .authorize_resource_commit(&context, cap::DATA_PUBLISH)
            .await?;
        let owner = Self::data_identity(&caller)?;
        let key = mutation_key(mutation)?;
        // Published records end with this authenticated producer session, so a later session
        // must not replay a receipt whose record was already purged at disconnect.
        let session_key = format!("{}:{key}", caller.session_id);
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "data publication is append-only; expected_revision must be 0",
            ));
        }
        let durability = StorageDurability::try_from(request.durability)
            .map_err(|_| Status::invalid_argument("unknown data durability"))?;
        if durability == StorageDurability::Unspecified {
            return Err(Status::invalid_argument(
                "data durability must be session-only or persistent",
            ));
        }
        // A durable publication needs both grants. A missing one is an explicit failure, never
        // a session-only fallback.
        let persistent = durability == StorageDurability::Persistent;
        if persistent && !caller.has(cap::STORAGE_PERSIST) {
            return Err(Status::permission_denied(
                "durable shared-data publication requires storage.persist",
            ));
        }
        let has_opaque_payload = !request.opaque_payload.is_empty();
        if request.value.is_some() == has_opaque_payload {
            return Err(Status::invalid_argument(
                "provide exactly one of value or opaque_payload",
            ));
        }
        if request
            .value
            .as_ref()
            .is_some_and(|value| value.fields.is_empty())
        {
            return Err(Status::invalid_argument(
                "value must be a non-empty Struct; use opaque_payload for opaque bytes",
            ));
        }
        let fields = request
            .value
            .as_ref()
            .map(struct_to_json)
            .transpose()?
            .unwrap_or_default();
        let payload_json = if request.value.is_some() {
            JsonValue::Object(fields.clone()).to_string()
        } else {
            String::new()
        };
        let now = unix_now()?;
        if request.ttl_seconds > super::shared_data::MAX_TTL_SECONDS {
            return Err(Status::invalid_argument(
                "ttl_seconds is outside the supported range",
            ));
        }
        let expires_at = if request.ttl_seconds == 0 {
            None
        } else {
            let seconds = i64::try_from(request.ttl_seconds)
                .map_err(|_| Status::invalid_argument("ttl_seconds is too large"))?;
            Some(
                now.checked_add(seconds)
                    .ok_or_else(|| Status::invalid_argument("ttl_seconds is too large"))?,
            )
        };
        let hash = mutations::request_hash_with_bytes(
            &[
                "data.publish",
                if request.value.is_some() {
                    "struct"
                } else {
                    "opaque"
                },
                &request.dataset_id,
                &request.schema_id,
                &request.schema_version.to_string(),
                &request.ttl_seconds.to_string(),
                &payload_json,
                &request.resource_id,
                &request.observed_at.to_string(),
                &request.sequence.to_string(),
                &request.source_observation_id,
                &request.durability.to_string(),
                &mutation.expected_revision.to_string(),
            ],
            &request.opaque_payload,
        );

        let _serial = self.mutations.lock.lock().await;
        if let Some((original, expires_at)) =
            self.mutations.replay_with(&caller, &session_key, hash)?
        {
            return Ok(Response::new(DataMutationReply {
                record_id: original.resource_id,
                revision: original.revision,
                expires_at,
                outcome: original.outcome,
                durability: durability as i32,
            }));
        }
        let current = self
            .providers
            .authorize_resource(Some(&context), cap::DATA_PUBLISH)?;
        if current.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "plugin permissions changed; retry the publication",
            ));
        }
        let publication = super::shared_data::Publication {
            owner,
            dataset_id: request.dataset_id,
            category: None,
            class: super::sharing_policy::SharingClass::Custom,
            schema_id: request.schema_id,
            schema_version: request.schema_version,
            fields,
            opaque_payload: request.opaque_payload,
            resource_id: request.resource_id,
            observed_at: request.observed_at,
            sequence: request.sequence,
            source_observation_id: request.source_observation_id,
            session_id: Some(caller.session_id.clone()),
            publisher: Some(super::shared_data::PublisherMetadata {
                declared_developer: caller.declared_developer.clone(),
                origin: caller.origin.clone(),
                package_version: caller.package_version.clone(),
                package_digest: caller.package_digest.clone(),
            }),
            expires_at,
        };
        // Both modes accept the same payload, provenance and expiry.
        DataBroker::validate(&publication, now).map_err(data_broker_status)?;

        let (record_id, revision, duplicate, expiry) = if persistent {
            let namespace = caller.namespace.as_ref().ok_or_else(|| {
                Status::permission_denied(
                    "shared plugin data is available only to authenticated third-party plugins",
                )
            })?;
            let scope = OwnerScope::new(*namespace.storage_owner().key(), caller.profile_epoch)
                .map_err(|_| Status::failed_precondition("resource context is stale"))?;
            let provenance = RecordProvenance {
                source_plugin_id: publication.owner.plugin_id.clone(),
                source_publisher_fingerprint: publication.owner.publisher_fingerprint.clone(),
                source_profile_id: publication.owner.profile_id.clone(),
                declared_developer: caller.declared_developer.clone(),
                origin: caller.origin.clone(),
                package_version: caller.package_version.clone(),
                package_digest: caller.package_digest.clone(),
            };
            let record = self
                .shared_pool
                .persist(DurableWrite {
                    owner: scope,
                    dataset_id: publication.dataset_id.clone(),
                    schema_id: publication.schema_id.clone(),
                    schema_version: publication.schema_version,
                    fields_json: payload_json,
                    opaque_payload: publication.opaque_payload.clone(),
                    resource_id: (!publication.resource_id.is_empty())
                        .then(|| publication.resource_id.clone()),
                    observed_at: publication.observed_at,
                    sequence: publication.sequence,
                    source_observation_id: publication.source_observation_id.clone(),
                    expires_at,
                    now,
                    provenance,
                })
                .await?;
            let (record_id, revision, expiry) = (
                record.record_id.clone(),
                record.revision,
                record.expires_at.unwrap_or_default(),
            );
            // The pool is the source of truth; the broker only advances its revision and tells
            // current watchers, so a list revision hands off to a watch across both sources.
            self.data.note_durable(record, now);
            (record_id, revision, false, expiry)
        } else {
            let receipt = match self.data.publish(publication, now) {
                Ok(receipt) => receipt,
                Err(BrokerError::SequenceConflict(revision)) => {
                    return Ok(Response::new(DataMutationReply {
                        revision,
                        outcome: MutationOutcome::RevisionConflict as i32,
                        durability: durability as i32,
                        ..Default::default()
                    }));
                }
                Err(error) => return Err(data_broker_status(error)),
            };
            (
                receipt.record_id,
                receipt.revision,
                receipt.duplicate,
                expires_at.unwrap_or_default(),
            )
        };
        let outcome = if duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        let generic_reply = mutations::reply(&record_id, revision, outcome);
        self.mutations
            .remember_with(&caller, &session_key, hash, &generic_reply, expiry);
        Ok(Response::new(DataMutationReply {
            record_id,
            revision,
            expires_at: expiry,
            outcome: outcome as i32,
            durability: durability as i32,
        }))
    }

    async fn data_get(
        &self,
        request: Request<DataGetRequest>,
    ) -> Result<Response<DataGetReply>, Status> {
        let request = request.into_inner();
        if request.dataset_id.trim().is_empty()
            || request.record_id.trim().is_empty()
            || request.record_id.len() > 256
        {
            return Err(Status::invalid_argument(
                "dataset_id and record_id are required",
            ));
        }
        let caller = self
            .providers
            .authorize_resource_session(request.context.as_ref())?;
        let identity = Self::data_identity(&caller)?;
        // The producer may be offline: a durable record is read from the pool, while a session
        // record still comes from the broker. Both pass the same grant and projection checks.
        let now = unix_now()?;
        let durable = self
            .shared_pool
            .get_any(request.record_id.clone(), now)
            .await?;
        let result = self
            .data
            .query_data_with_durable(
                QueryRequest {
                    caller: identity,
                    dataset_id: request.dataset_id,
                    record_id: Some(request.record_id.clone()),
                    category: None,
                    selector: request.common_data_categories,
                    delivery: Delivery::Query,
                    page_size: 1,
                    cursor: None,
                },
                durable.into_iter().collect(),
                self.shared_authorizer.as_ref(),
                now,
            )
            .map_err(data_broker_status)?;
        let record = result
            .records
            .into_iter()
            .find(|record| record.record_id() == request.record_id)
            .ok_or_else(|| Status::not_found("data record not found"))?;
        Ok(Response::new(DataGetReply {
            record: Some(data_record(&record)),
        }))
    }

    async fn data_list(
        &self,
        request: Request<DataListRequest>,
    ) -> Result<Response<DataListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        if request.dataset_id.trim().is_empty() || request.dataset_id.len() > 256 {
            return Err(Status::invalid_argument("dataset_id is invalid"));
        }
        if !request.filter.trim().is_empty() {
            return Err(Status::invalid_argument(
                "Data.List filter is not available for this schema",
            ));
        }
        let caller = self
            .providers
            .authorize_resource_session(page.context.as_ref())?;
        let identity = Self::data_identity(&caller)?;
        let selector = match (request.common_data_categories, page.common_data_categories) {
            (Some(outer), Some(inner)) if outer != inner => {
                return Err(Status::invalid_argument("Data.List selectors disagree"));
            }
            (Some(outer), _) => Some(outer),
            (_, Some(inner)) => Some(inner),
            _ => None,
        };
        let cursor = self.take_data_cursor(&page.cursor, &caller.session_id)?;
        let page_size = match page.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let now = unix_now()?;
        // One page spans both sources, in the same receipt order, whether or not the producer is
        // still connected. The persisted pool sequence seeds the broker first, so a restarted
        // host hands the durable revision to a subsequent watch instead of restarting at zero.
        let (durable, pool_revision) = self
            .shared_pool
            .list_dataset_any(request.dataset_id.clone(), now)
            .await?;
        self.data.observe_pool_revision(pool_revision);
        let result = self
            .data
            .query_data_with_durable(
                QueryRequest {
                    caller: identity,
                    dataset_id: request.dataset_id,
                    record_id: None,
                    category: None,
                    selector,
                    delivery: Delivery::History,
                    page_size,
                    cursor,
                },
                durable,
                self.shared_authorizer.as_ref(),
                now,
            )
            .map_err(data_broker_status)?;
        let next_cursor = match result.next_cursor {
            Some(cursor) => self.issue_data_cursor(&caller.session_id, cursor)?,
            None => String::new(),
        };
        Ok(Response::new(DataListReply {
            records: result.records.iter().map(data_record).collect(),
            next_cursor,
            revision: result.revision,
        }))
    }

    async fn data_watch(
        &self,
        request: Request<DataWatchRequest>,
    ) -> Result<Response<Self::DataWatchStream>, Status> {
        let request = request.into_inner();
        if request.dataset_id.trim().is_empty() || request.dataset_id.len() > 256 {
            return Err(Status::invalid_argument("dataset_id is invalid"));
        }
        let context = request.context.clone();
        let caller = self
            .providers
            .authorize_resource_session(context.as_ref())?;
        let identity = Self::data_identity(&caller)?;
        self.data.expire(unix_now()?);
        // A watch positions against the persisted durable sequence, so the revision a `DataList`
        // returned after a restart is accepted and an older position is rejected as stale even
        // though the in-memory broker started empty.
        let pool_revision = self.shared_pool.pool_revision().await?;
        self.data.observe_pool_revision(pool_revision);
        let subscription = self
            .data
            .subscribe(
                QueryRequest {
                    caller: identity,
                    dataset_id: request.dataset_id,
                    record_id: None,
                    category: None,
                    selector: request.common_data_categories,
                    delivery: Delivery::Live,
                    page_size: DEFAULT_PAGE_SIZE,
                    cursor: None,
                },
                request.after_revision,
                WATCH_QUEUE,
                self.shared_authorizer.clone(),
            )
            .map_err(data_broker_status)?;
        let slot = self.watch_slot(&caller.session_id)?;
        let providers = self.providers.clone();
        let grant_revision = caller.grant_revision;
        // Keep pending records in the broker queue until the response stream polls them.
        // The broker rechecks the sharing revision when releasing each queued projection.
        let stream = futures_util::stream::unfold(
            Some((subscription, slot, providers, context)),
            move |state| async move {
                let (subscription, slot, providers, context) = state?;
                loop {
                    let received = tokio::time::timeout(
                        WATCH_RECHECK,
                        subscription.recv(unix_now().unwrap_or(0)),
                    )
                    .await;
                    match providers.authorize_resource_session(context.as_ref()) {
                        Ok(active) if active.grant_revision == grant_revision => {}
                        Ok(_) => {
                            return Some((
                                Err(Status::failed_precondition(
                                    "plugin permissions changed; list and watch again",
                                )),
                                None,
                            ));
                        }
                        Err(status) => return Some((Err(status), None)),
                    }
                    let record = match received {
                        Ok(Ok(Some(record))) => record,
                        Ok(Ok(None)) | Err(_) => continue,
                        Ok(Err(ended)) => {
                            let status = match ended {
                                SubscriptionEnd::Revoked => Status::failed_precondition(
                                    "data sharing permissions changed; list and watch again",
                                ),
                                SubscriptionEnd::FellBehind => Status::resource_exhausted(
                                    "data watch fell behind; list and watch again",
                                ),
                            };
                            return Some((Err(status), None));
                        }
                    };
                    let event = DataEvent {
                        revision: record.revision(),
                        event_type: "published".into(),
                        record: Some(data_record(&record)),
                    };
                    return Some((Ok(event), Some((subscription, slot, providers, context))));
                }
            },
        );
        Ok(Response::new(Box::pin(stream)))
    }

    async fn metadata_update(
        &self,
        request: Request<MetadataUpdateRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.update_metadata(request).await
    }

    async fn favorites_list(
        &self,
        request: Request<FavoritesListRequest>,
    ) -> Result<Response<FavoritesListReply>, Status> {
        self.list_favorites(request).await
    }

    async fn favorites_set(
        &self,
        request: Request<FavoritesSetRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.set_favorite(request).await
    }

    async fn matching_search(
        &self,
        request: Request<MatchingSearchRequest>,
    ) -> Result<Response<MatchingSearchReply>, Status> {
        self.search_catalog(request).await
    }

    async fn matching_assign(
        &self,
        request: Request<MatchingAssignRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.assign_match(request).await
    }

    async fn artwork_read(
        &self,
        request: Request<ArtworkReadRequest>,
    ) -> Result<Response<ArtworkReadReply>, Status> {
        self.read_artwork(request).await
    }

    async fn artwork_set(
        &self,
        request: Request<ArtworkSetRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.set_artwork(request).await
    }

    async fn storage_get(
        &self,
        request: Request<StorageGetRequest>,
    ) -> Result<Response<StorageGetReply>, Status> {
        self.get_stored(request).await
    }

    async fn storage_put(
        &self,
        request: Request<StoragePutRequest>,
    ) -> Result<Response<StorageMutationReply>, Status> {
        self.put_stored(request).await
    }

    async fn storage_delete(
        &self,
        request: Request<StorageDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.delete_stored(request).await
    }

    async fn files_request_access(
        &self,
        request: Request<FilesRequestAccessRequest>,
    ) -> Result<Response<FileAccessReply>, Status> {
        self.request_files_access(request).await
    }

    async fn files_list(
        &self,
        request: Request<FilesListRequest>,
    ) -> Result<Response<FilesListReply>, Status> {
        self.list_files(request).await
    }

    async fn files_read(
        &self,
        request: Request<FilesReadRequest>,
    ) -> Result<Response<FileReadReply>, Status> {
        self.read_file(request).await
    }

    async fn files_write(
        &self,
        request: Request<FilesWriteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.write_file(request).await
    }

    async fn files_delete(
        &self,
        request: Request<FilesDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.delete_file(request).await
    }

    async fn network_fetch(
        &self,
        request: Request<NetworkFetchRequest>,
    ) -> Result<Response<NetworkFetchReply>, Status> {
        self.fetch_url(request).await
    }

    async fn notifications_publish(
        &self,
        request: Request<NotificationPublishRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.publish_notification(request).await
    }

    async fn games_request_launch(
        &self,
        request: Request<GameLaunchRequest>,
    ) -> Result<Response<GameLaunchReply>, Status> {
        self.request_launch(request).await
    }

    async fn games_propose_custom_launch(
        &self,
        request: Request<CustomLaunchRequest>,
    ) -> Result<Response<CustomLaunchReply>, Status> {
        self.propose_custom_launch(request).await
    }

    async fn games_request_custom_launch(
        &self,
        request: Request<CustomLaunchRequest>,
    ) -> Result<Response<CustomLaunchReply>, Status> {
        self.request_custom_launch(request).await
    }

    async fn games_remove_custom_launch(
        &self,
        request: Request<CustomLaunchRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.remove_custom_launch(request).await
    }

    async fn games_request_install(
        &self,
        request: Request<GameInstallRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.request_install(request).await
    }

    async fn jobs_get(
        &self,
        request: Request<JobGetRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.get_job(request).await
    }

    async fn jobs_watch(
        &self,
        request: Request<JobWatchRequest>,
    ) -> Result<Response<Self::JobsWatchStream>, Status> {
        self.watch_job(request).await
    }

    async fn jobs_cancel(
        &self,
        request: Request<JobCancelRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.cancel_job(request).await
    }

    async fn libraries_list(
        &self,
        request: Request<DomainListRequest>,
    ) -> Result<Response<LibrariesListReply>, Status> {
        self.list_libraries(request).await
    }

    async fn libraries_get(
        &self,
        request: Request<DomainGetRequest>,
    ) -> Result<Response<LibraryRecordReply>, Status> {
        self.get_library(request).await
    }

    async fn libraries_watch(
        &self,
        request: Request<DomainWatchRequest>,
    ) -> Result<Response<Self::LibrariesWatchStream>, Status> {
        self.watch_libraries(request).await
    }

    async fn libraries_create(
        &self,
        request: Request<LibraryCreateRequest>,
    ) -> Result<Response<LibraryMutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(r.mutation, libraries::Edit::Create(r.name, r.description))
            .await?;
        Ok(Response::new(LibraryMutationReply {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome: reply.outcome,
        }))
    }

    async fn libraries_update(
        &self,
        request: Request<LibraryUpdateRequest>,
    ) -> Result<Response<LibraryMutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(
                r.mutation,
                libraries::Edit::Update(r.library_id, r.name, r.description, r.field_mask),
            )
            .await?;
        Ok(Response::new(LibraryMutationReply {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome: reply.outcome,
        }))
    }

    async fn libraries_remove(
        &self,
        request: Request<DomainRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(r.mutation, libraries::Edit::Remove(r.resource_id))
            .await?;
        Ok(Response::new(reply))
    }

    async fn libraries_add_root(
        &self,
        request: Request<LibraryRootAddRequest>,
    ) -> Result<Response<LibraryMutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(
                r.mutation,
                libraries::Edit::Add(
                    r.library_id,
                    r.approved_root_handle,
                    r.display_name,
                    r.declared_locations,
                ),
            )
            .await?;
        Ok(Response::new(LibraryMutationReply {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome: reply.outcome,
        }))
    }

    async fn libraries_update_root(
        &self,
        request: Request<LibraryRootUpdateRequest>,
    ) -> Result<Response<LibraryMutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(
                r.mutation,
                libraries::Edit::Root(
                    r.library_id,
                    r.root_id,
                    r.approved_root_handle,
                    r.display_name,
                    r.declared_locations,
                    r.field_mask,
                    r.watching,
                ),
            )
            .await?;
        Ok(Response::new(LibraryMutationReply {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome: reply.outcome,
        }))
    }

    async fn libraries_remove_root(
        &self,
        request: Request<LibraryRootRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let r = request.into_inner();
        let reply = self
            .edit_library(
                r.mutation,
                libraries::Edit::RemoveRoot(r.library_id, r.root_id),
            )
            .await?;
        Ok(Response::new(reply))
    }

    async fn backups_list(
        &self,
        request: Request<BackupsListRequest>,
    ) -> Result<Response<BackupsListReply>, Status> {
        self.list_backups(request).await
    }
    async fn backups_request_create(
        &self,
        request: Request<BackupCreateRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.create_backup(request).await
    }
    async fn backups_request_restore(
        &self,
        request: Request<BackupRestoreRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.restore_backup(request).await
    }

    async fn tools_list(
        &self,
        request: Request<ToolsListRequest>,
    ) -> Result<Response<ToolsListReply>, Status> {
        self.list_tools(request).await
    }
    async fn tools_request_apply(
        &self,
        request: Request<ToolApplyRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let r = request.into_inner();
        self.request_tool_action(r.mutation, r.tool_id, r.game_id, r.user_intent_id, false)
            .await
    }
    async fn tools_request_remove(
        &self,
        request: Request<ToolRemoveRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let r = request.into_inner();
        self.request_tool_action(r.mutation, r.tool_id, r.game_id, r.user_intent_id, true)
            .await
    }
    async fn ports_list(
        &self,
        request: Request<PortsListRequest>,
    ) -> Result<Response<PortsListReply>, Status> {
        self.list_ports(request).await
    }
    async fn ports_detail(
        &self,
        request: Request<PortDetailRequest>,
    ) -> Result<Response<PortDetailReply>, Status> {
        self.detail_port(request).await
    }
    async fn ports_request_install(
        &self,
        request: Request<PortInstallRequest>,
    ) -> Result<Response<JobReply>, Status> {
        self.install_port(request).await
    }
    async fn runtimes_list(
        &self,
        request: Request<DomainListRequest>,
    ) -> Result<Response<RuntimesListReply>, Status> {
        self.list_runtimes(request).await
    }
    async fn runtimes_get(
        &self,
        request: Request<DomainGetRequest>,
    ) -> Result<Response<RuntimeRecordReply>, Status> {
        self.get_runtime(request).await
    }
    async fn runtimes_watch(
        &self,
        request: Request<DomainWatchRequest>,
    ) -> Result<Response<Self::RuntimesWatchStream>, Status> {
        self.watch_runtimes(request).await
    }
    async fn runtimes_register(
        &self,
        request: Request<RuntimeRegisterRequest>,
    ) -> Result<Response<RuntimeMutationReply>, Status> {
        self.register_runtime(request).await
    }
    async fn runtimes_request_access(
        &self,
        request: Request<RuntimeRequestAccessRequest>,
    ) -> Result<Response<RuntimeMutationReply>, Status> {
        self.request_runtime_access(request).await
    }
    async fn runtimes_update(
        &self,
        request: Request<RuntimeUpdateRequest>,
    ) -> Result<Response<RuntimeMutationReply>, Status> {
        self.update_runtime(request).await
    }
    async fn runtimes_remove(
        &self,
        request: Request<DomainRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.remove_runtime(request).await
    }
    async fn games_register_content(
        &self,
        request: Request<ContentRegistrationRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.register_content(request).await
    }
    async fn storage_diagnostics_publish_benchmark(
        &self,
        request: Request<BenchmarkReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        self.publish_benchmark(request).await
    }
    async fn storage_diagnostics_publish_smart(
        &self,
        request: Request<SmartReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        self.publish_smart(request).await
    }
    async fn computer_publish_hardware(
        &self,
        request: Request<HardwareReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        self.publish_hardware(request).await
    }
    async fn game_presence_publish(
        &self,
        request: Request<PresencePublishRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        self.publish_presence(request).await
    }
    async fn game_presence_clear(
        &self,
        request: Request<PresenceClearRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.clear_presence(request).await
    }
    async fn game_performance_publish_samples(
        &self,
        request: Request<PerformanceSamplesRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        self.publish_performance_samples(request).await
    }
    async fn game_performance_end_series(
        &self,
        request: Request<PerformanceEndSeriesRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.end_performance_series(request).await
    }
    async fn persistence_get_usage(
        &self,
        request: Request<PersistenceUsageRequest>,
    ) -> Result<Response<PersistenceUsageReply>, Status> {
        self.persistence_usage(request).await
    }
    async fn persistence_list_entries(
        &self,
        request: Request<PersistenceListRequest>,
    ) -> Result<Response<PersistenceListReply>, Status> {
        self.persistence_list(request).await
    }
    async fn persistence_delete_entries(
        &self,
        request: Request<PersistenceDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.persistence_delete(request).await
    }
    async fn persistence_clear(
        &self,
        request: Request<PersistenceClearRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.persistence_clear_entries(request).await
    }
    async fn persistence_set_retention_policy(
        &self,
        request: Request<RetentionPolicyRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        self.persistence_set_policy(request).await
    }
}

#[cfg(all(test, unix))]
mod tests;
