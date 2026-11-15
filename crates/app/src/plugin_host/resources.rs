//! The API 1.6 `ResourceHost` service. Library list/get/watch are served from the host-owned
//! [`LibraryFeed`], display metadata and favorites through the engine's [`LibraryHost`]; every
//! other method answers `UNIMPLEMENTED` until its adapter exists, and
//! `DescribeApi` advertises only what is served.
//!
//! Calls are plugin-initiated: they carry the session's resource context from `OpenSession`,
//! which [`ProviderApi::authorize_resource`] checks against the session, profile and grants.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::Stream;
use gamenode_plugin_protocol::{capability as cap, proto::*};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

mod artwork;
mod backups;
mod files;
mod games;
mod jobs;
mod libraries;
mod locations;
mod matching;
mod mutations;
mod network;
mod notifications;
mod saves;
mod storage;
mod tools;

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
use super::storage::PluginStorage;

/// Open `LibraryWatch` streams allowed per plugin session.
pub const MAX_WATCHES_PER_SESSION: usize = 8;
/// How often an idle watch rechecks its authorization.
const WATCH_RECHECK: Duration = Duration::from_secs(5);
const WATCH_QUEUE: usize = 16;
const MAX_PROVIDER_ID_BYTES: usize = 128;

/// The ResourceHost methods this build serves, for `DescribeApi`.
pub const SERVED: &[Served] = &[
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
        "GamesRequestInstall",
        cap::GAMES_INSTALL,
        false,
        OWN_OR_TARGET,
    ),
    Served::new("JobsGet", UNDERLYING, false, OWN_JOBS),
    Served::new("JobsWatch", UNDERLYING, false, OWN_JOBS),
    Served::new("JobsCancel", UNDERLYING, false, OWN_JOBS),
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
const OWN_NOTICES: &str = "plain text attributed to the calling plugin";
const APPROVED_ORIGINS: &str = "exact HTTPS origins the user approved for this plugin";
const APPROVED_FOLDERS: &str = "folder the user approved for this plugin and installation";

/// The `Saves` service methods this build serves, for `DescribeApi`.
pub const SAVES_SERVED: &[Served] = saves::SERVED;

/// The `Locations` service methods this build serves, for `DescribeApi`.
pub const LOCATIONS_SERVED: &[Served] = locations::SERVED;

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
}

impl ResourceApi {
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
        }
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

fn not_served(method: &str) -> Status {
    Status::unimplemented(format!(
        "ResourceHost.{method} is not available in this GameNode build"
    ))
}

fn stale_cursor() -> Status {
    Status::failed_precondition("library cursor is stale; restart the list")
}

fn event(change: &Change) -> LibraryEvent {
    LibraryEvent {
        revision: change.revision,
        event_type: change.kind.as_str().to_owned(),
        entry: Some(change.entry.to_proto()),
    }
}

/// Answers async_trait's expanded signature directly, so the not-yet-served methods can be
/// listed instead of written out one by one.
macro_rules! not_served {
    ($($method:ident($request:ty) -> $reply:ty;)*) => {
        $(
            fn $method<'life0, 'async_trait>(
                &'life0 self,
                _request: Request<$request>,
            ) -> Pin<
                Box<
                    dyn std::future::Future<Output = Result<Response<$reply>, Status>>
                        + Send
                        + 'async_trait,
                >,
            >
            where
                'life0: 'async_trait,
                Self: 'async_trait,
            {
                Box::pin(async move { Err(not_served(stringify!($method))) })
            }
        )*
    };
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
        no_common_data(page.common_data_categories.as_ref())?;
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
        Ok(Response::new(LibraryListReply {
            entries: entries.iter().map(|entry| entry.to_proto()).collect(),
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
        no_common_data(request.common_data_categories.as_ref())?;
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
        Ok(Response::new(LibraryGetReply {
            entry: Some(entry.to_proto()),
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
        no_common_data(request.common_data_categories.as_ref())?;
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
        let provider_filter = request.provider_id;
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
                Self::scope(&current, &provider_filter)
            };
            let deliver = |change: &Change, scope: &Scope| {
                scope.admits(&change.entry).then(|| Ok(event(change)))
            };
            for change in &backlog {
                let scope = match check() {
                    Ok(scope) => scope,
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        return;
                    }
                };
                if let Some(item) = deliver(change, &scope)
                    && tx.send(item).await.is_err()
                {
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
                if let Some(item) = change.as_ref().and_then(|change| deliver(change, &scope))
                    && tx.send(item).await.is_err()
                {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
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
    not_served! {
        games_propose_custom_launch(CustomLaunchRequest) -> CustomLaunchReply;
        games_request_custom_launch(CustomLaunchRequest) -> CustomLaunchReply;
        games_remove_custom_launch(CustomLaunchRemoveRequest) -> MutationReply;
        runtimes_list(DomainListRequest) -> RuntimesListReply;
        runtimes_get(DomainGetRequest) -> RuntimeRecordReply;
        runtimes_watch(DomainWatchRequest) -> Self::RuntimesWatchStream;
        runtimes_register(RuntimeRegisterRequest) -> RuntimeMutationReply;
        runtimes_update(RuntimeUpdateRequest) -> RuntimeMutationReply;
        runtimes_remove(DomainRemoveRequest) -> MutationReply;
        games_register_content(ContentRegistrationRequest) -> MutationReply;
        storage_diagnostics_publish_benchmark(BenchmarkReportRequest) -> ReportMutationReply;
        storage_diagnostics_publish_smart(SmartReportRequest) -> ReportMutationReply;
        computer_publish_hardware(HardwareReportRequest) -> ReportMutationReply;
        game_presence_publish(PresencePublishRequest) -> ReportMutationReply;
        game_presence_clear(PresenceClearRequest) -> MutationReply;
        game_performance_publish_samples(PerformanceSamplesRequest) -> ReportMutationReply;
        game_performance_end_series(PerformanceEndSeriesRequest) -> MutationReply;
        data_publish(DataPublishRequest) -> DataMutationReply;
        data_get(DataGetRequest) -> DataGetReply;
        data_list(DataListRequest) -> DataListReply;
        data_watch(DataWatchRequest) -> Self::DataWatchStream;
        persistence_get_usage(PersistenceUsageRequest) -> PersistenceUsageReply;
        persistence_list_entries(PersistenceListRequest) -> PersistenceListReply;
        persistence_delete_entries(PersistenceDeleteRequest) -> MutationReply;
        persistence_clear(PersistenceClearRequest) -> MutationReply;
        persistence_set_retention_policy(RetentionPolicyRequest) -> MutationReply;
        ports_list(PortsListRequest) -> PortsListReply;
        ports_detail(PortDetailRequest) -> PortDetailReply;
        ports_request_install(PortInstallRequest) -> JobReply;
    }
}

#[cfg(all(test, unix))]
mod tests;
