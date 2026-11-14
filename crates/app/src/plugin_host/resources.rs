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

mod mutations;

use super::library::{
    Change, CursorBinding, CursorError, DEFAULT_PAGE_SIZE, LibraryFeed, MAX_PAGE_SIZE,
    MAX_SEARCH_BYTES, ResumeError, Scope,
};
use super::provider::{ProviderApi, ResourceCaller};

/// Open `LibraryWatch` streams allowed per plugin session.
pub const MAX_WATCHES_PER_SESSION: usize = 8;
/// How often an idle watch rechecks its authorization.
const WATCH_RECHECK: Duration = Duration::from_secs(5);
const WATCH_QUEUE: usize = 16;
const MAX_PROVIDER_ID_BYTES: usize = 128;

/// The ResourceHost methods this build serves, for `DescribeApi`.
pub const SERVED: &[Served] = &[
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
];

const READ_SCOPE: &str = "own provider; library.read.all for every provider";
const OWN_ENTRIES: &str = "own provider entries";

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
}

impl ResourceApi {
    pub fn new(providers: ProviderApi, library: Arc<LibraryFeed>) -> Self {
        Self {
            providers,
            library,
            watches: Arc::default(),
            mutations: Arc::default(),
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
                "too many library watches for this session",
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

    not_served! {
        matching_search(MatchingSearchRequest) -> MatchingSearchReply;
        matching_assign(MatchingAssignRequest) -> MutationReply;
        artwork_read(ArtworkReadRequest) -> ArtworkReadReply;
        artwork_set(ArtworkSetRequest) -> MutationReply;
        games_request_launch(GameLaunchRequest) -> GameLaunchReply;
        games_propose_custom_launch(CustomLaunchRequest) -> CustomLaunchReply;
        games_request_custom_launch(CustomLaunchRequest) -> CustomLaunchReply;
        games_remove_custom_launch(CustomLaunchRemoveRequest) -> MutationReply;
        games_request_install(GameInstallRequest) -> JobReply;
        storage_get(StorageGetRequest) -> StorageGetReply;
        storage_put(StoragePutRequest) -> StorageMutationReply;
        storage_delete(StorageDeleteRequest) -> MutationReply;
        files_request_access(FilesRequestAccessRequest) -> FileAccessReply;
        files_list(FilesListRequest) -> FilesListReply;
        files_read(FilesReadRequest) -> FileReadReply;
        files_write(FilesWriteRequest) -> MutationReply;
        files_delete(FilesDeleteRequest) -> MutationReply;
        network_fetch(NetworkFetchRequest) -> NetworkFetchReply;
        notifications_publish(NotificationPublishRequest) -> MutationReply;
        backups_list(BackupsListRequest) -> BackupsListReply;
        backups_request_create(BackupCreateRequest) -> JobReply;
        backups_request_restore(BackupRestoreRequest) -> JobReply;
        libraries_list(DomainListRequest) -> LibrariesListReply;
        libraries_get(DomainGetRequest) -> LibraryRecordReply;
        libraries_watch(DomainWatchRequest) -> Self::LibrariesWatchStream;
        libraries_create(LibraryCreateRequest) -> LibraryMutationReply;
        libraries_update(LibraryUpdateRequest) -> LibraryMutationReply;
        libraries_remove(DomainRemoveRequest) -> MutationReply;
        libraries_add_root(LibraryRootAddRequest) -> LibraryMutationReply;
        libraries_update_root(LibraryRootUpdateRequest) -> LibraryMutationReply;
        libraries_remove_root(LibraryRootRemoveRequest) -> MutationReply;
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
        tools_list(ToolsListRequest) -> ToolsListReply;
        tools_request_apply(ToolApplyRequest) -> JobReply;
        tools_request_remove(ToolRemoveRequest) -> JobReply;
        ports_list(PortsListRequest) -> PortsListReply;
        ports_detail(PortDetailRequest) -> PortDetailReply;
        ports_request_install(PortInstallRequest) -> JobReply;
        jobs_get(JobGetRequest) -> JobReply;
        jobs_watch(JobWatchRequest) -> Self::JobsWatchStream;
        jobs_cancel(JobCancelRequest) -> JobReply;
    }
}

#[cfg(all(test, unix))]
mod tests;
