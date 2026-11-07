//! Local Client API shared by the CLI and the daemon host.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio_stream::{StreamExt, wrappers::UnixListenerStream};
use tonic::{Request, Response, Status, transport::Server};

pub mod proto {
    tonic::include_proto!("gamenode.client.v1");
}

static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct ScanOperation {
    id: String,
    state: String,
    result_json: String,
}

use crate::engine::Engine;
use proto::client_server::{Client, ClientServer};
use proto::*;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

fn scan_result_json(engine: &Engine) -> String {
    let snapshot = engine.snapshot();
    let directories = engine.installation_directories();
    let value = snapshot
        .library
        .map(|library| {
            let entries = library
                .rows
                .iter()
                .map(|row| {
                    let (provider, store_id) =
                        row.key.split_once(':').unwrap_or((&row.provider, &row.key));
                    serde_json::json!({
                        "entryKey": row.key,
                        "provider": provider,
                        "storeId": store_id,
                        "title": row.title,
                        "installDir": directories.get(&row.key).cloned().unwrap_or_default(),
                        "match": row.status,
                        "running": row.running,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "access": format!("{:?}", library.access).to_ascii_lowercase(),
                "entries": entries,
                "failedProviders": library.failed_providers,
            "persistenceFailures": engine.scan_persistence_status().1,
            })
        })
        .unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&value).unwrap_or_else(|_| "null".into())
}

#[derive(Clone)]
pub struct Service {
    engine: Engine,
    stop: Arc<AtomicBool>,
    operations: Arc<Mutex<std::collections::VecDeque<ScanOperation>>>,
}
impl Service {
    pub fn new(engine: Engine, stop: Arc<AtomicBool>) -> Self {
        Self {
            engine,
            stop,
            operations: Arc::default(),
        }
    }
}

#[tonic::async_trait]
impl Client for Service {
    async fn get_status(&self, _: Request<Empty>) -> Result<Response<DaemonStatus>, Status> {
        Ok(Response::new(DaemonStatus {
            version: env!("CARGO_PKG_VERSION").into(),
            platform: std::env::consts::OS.into(),
        }))
    }
    async fn get_auth_status(&self, _: Request<Empty>) -> Result<Response<AuthStatus>, Status> {
        let snapshot = self.engine.snapshot();
        let (state, account) = match snapshot.auth {
            gamenode_core::AuthView::SignedIn { user } => {
                ("signed_in", user.display_name().to_owned())
            }
            gamenode_core::AuthView::Starting => ("starting", String::new()),
            gamenode_core::AuthView::Pending { .. } => ("pending", String::new()),
            gamenode_core::AuthView::Verifying => ("verifying", String::new()),
            gamenode_core::AuthView::RestoreFailed => ("restore_failed", String::new()),
            gamenode_core::AuthView::SigningOut => ("signing_out", String::new()),
            gamenode_core::AuthView::SignedOut => ("signed_out", String::new()),
        };
        Ok(Response::new(AuthStatus {
            state: state.into(),
            account,
            sync_enabled: snapshot.sync.enabled,
            playtime_source: format!("{:?}", snapshot.playtime_source).to_ascii_lowercase(),
        }))
    }
    async fn start_login(
        &self,
        request: Request<StartLoginRequest>,
    ) -> Result<Response<AuthStatus>, Status> {
        use gamenode_core::Input;
        let provider = match request.into_inner().provider.to_ascii_lowercase().as_str() {
            "google" => gamenode_core::Provider::Google,
            "discord" => gamenode_core::Provider::Discord,
            "twitter" => gamenode_core::Provider::Twitter,
            _ => {
                return Err(Status::invalid_argument(
                    "provider must be google, discord, or twitter",
                ));
            }
        };
        self.engine.dispatch(Input::StartSignIn(provider));
        self.get_auth_status(Request::new(Empty {})).await
    }
    async fn sign_out(&self, _: Request<Empty>) -> Result<Response<AuthStatus>, Status> {
        self.engine.dispatch(gamenode_core::Input::SignOut);
        self.get_auth_status(Request::new(Empty {})).await
    }
    async fn set_sync(
        &self,
        request: Request<SyncSettings>,
    ) -> Result<Response<ActionReply>, Status> {
        self.engine.set_sync(request.into_inner().enabled);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn set_playtime_source(
        &self,
        request: Request<PlaytimeSourceRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        let source = match request.into_inner().source.to_ascii_lowercase().as_str() {
            "store" => gamenode_core::playtime::PlaytimeSource::Store,
            "internal" => gamenode_core::playtime::PlaytimeSource::Internal,
            _ => return Err(Status::invalid_argument("source must be store or internal")),
        };
        self.engine.set_playtime_source(source);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn list_installations(
        &self,
        _: Request<Empty>,
    ) -> Result<Response<InstallationSnapshot>, Status> {
        let snapshot = self.engine.snapshot();
        if snapshot.library.is_none() {
            return Err(match snapshot.auth {
                gamenode_core::AuthView::SignedOut | gamenode_core::AuthView::RestoreFailed => {
                    Status::failed_precondition("sign in before accessing the local library")
                }
                _ => Status::unavailable("the local library is still loading"),
            });
        }
        let directories = self.engine.installation_directories();
        let mut result = InstallationSnapshot {
            access: "unverified".into(),
            entries: Vec::new(),
            scanning: false,
        };
        if let Some(library) = snapshot.library {
            result.access = format!("{:?}", library.access).to_ascii_lowercase();
            result.scanning = library.scanning;
            result.entries = library
                .rows
                .iter()
                .map(|row| {
                    let (provider, store_id) =
                        row.key.split_once(':').unwrap_or((&row.provider, &row.key));
                    Installation {
                        entry_key: row.key.clone(),
                        provider: provider.into(),
                        store_id: store_id.into(),
                        title: row.title.clone(),
                        install_dir: directories.get(&row.key).cloned().unwrap_or_default(),
                        r#match: serde_json::to_string(&row.status).unwrap_or_default(),
                        running: row.running,
                    }
                })
                .collect();
        }
        Ok(Response::new(result))
    }
    async fn start_scan(&self, _: Request<Empty>) -> Result<Response<ScanStatus>, Status> {
        if self.engine.snapshot().library.is_none() {
            return Err(Status::failed_precondition(
                "sign in before scanning local installations",
            ));
        }
        self.engine.reset_scan_persistence_failures();
        self.engine.rescan();
        let operation_id = format!(
            "{}-{}-{}",
            std::process::id(),
            unix_now(),
            NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)
        );
        {
            let mut operations = self
                .operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            operations.push_back(ScanOperation {
                id: operation_id.clone(),
                state: "running".into(),
                result_json: String::new(),
            });
            while operations.len() > 100 {
                operations.pop_front();
            }
        }
        let engine = self.engine.clone();
        let operations = self.operations.clone();
        let tracked_id = operation_id.clone();
        tokio::spawn(async move {
            loop {
                let snapshot = engine.snapshot();
                let (pending_writes, persistence_failures) = engine.scan_persistence_status();
                if pending_writes > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                let (state, result_json) = match snapshot.library.as_ref() {
                    None => ("cancelled", String::new()),
                    Some(library) if library.scanning => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                    Some(library) => (
                        if library.failed_providers.is_empty() && persistence_failures.is_empty() {
                            "completed"
                        } else {
                            "partial"
                        },
                        scan_result_json(&engine),
                    ),
                };
                let mut operations = operations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(operation) = operations.iter_mut().find(|op| op.id == tracked_id) {
                    operation.state = state.into();
                    operation.result_json = result_json;
                }
                break;
            }
        });
        Ok(Response::new(ScanStatus {
            started: true,
            operation_id,
        }))
    }
    async fn get_operation_status(
        &self,
        request: Request<OperationRequest>,
    ) -> Result<Response<OperationStatus>, Status> {
        let operation_id = request.into_inner().operation_id;
        let tracked = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|operation| operation.id == operation_id);
        if !tracked {
            return Err(Status::not_found(
                "scan operation not found in this daemon session",
            ));
        }
        let operation = self
            .operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|operation| operation.id == operation_id)
            .cloned()
            .ok_or_else(|| Status::not_found("scan operation expired"))?;
        Ok(Response::new(OperationStatus {
            operation_id,
            kind: "scan".into(),
            state: operation.state,
            result_json: operation.result_json,
        }))
    }
    async fn play_game(
        &self,
        request: Request<GameRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        let key = request.into_inner().entry_key;
        if !self.engine.installation_directories().contains_key(&key) {
            return Err(Status::not_found("installation not found"));
        }
        self.engine
            .open_entry(key, gamenode_core::library::LaunchKind::Play);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn install_game(
        &self,
        request: Request<GameRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        let key = request.into_inner().entry_key;
        if !self.engine.installation_directories().contains_key(&key) {
            return Err(Status::not_found("installation not found"));
        }
        self.engine
            .open_entry(key, gamenode_core::library::LaunchKind::Install);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn remove_game(
        &self,
        request: Request<GameRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        let key = request.into_inner().entry_key;
        if !self.engine.installation_directories().contains_key(&key) {
            return Err(Status::not_found("installation not found"));
        }
        self.engine.remove_entry(key, false);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn match_game(
        &self,
        request: Request<MatchRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        use gamenode_core::library::{Input, SelectMode};
        let request = request.into_inner();
        if !self
            .engine
            .installation_directories()
            .contains_key(&request.entry_key)
        {
            return Err(Status::not_found("installation not found"));
        }
        self.engine.library_input(Input::Select {
            key: request.entry_key,
            mode: SelectMode::Replace,
        });
        self.engine.library_input(Input::PickMatch {
            game_id: request.game_id,
            now: unix_now(),
        });
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn search_matches(
        &self,
        request: Request<SearchMatchesRequest>,
    ) -> Result<Response<SearchMatchesReply>, Status> {
        use gamenode_core::library::{Input, SelectMode};
        let request = request.into_inner();
        if !self
            .engine
            .installation_directories()
            .contains_key(&request.entry_key)
        {
            return Err(Status::not_found("installation not found"));
        }
        self.engine.library_input(Input::Select {
            key: request.entry_key,
            mode: SelectMode::Replace,
        });
        self.engine.library_input(Input::OpenMatch);
        self.engine
            .library_input(Input::SetMatchQuery(request.query));
        self.engine.library_input(Input::SearchMatch);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let snapshot = self.engine.snapshot();
            if let Some(dialog) = snapshot
                .library
                .as_ref()
                .and_then(|view| view.match_dialog.as_ref())
            {
                if !dialog.searching {
                    let results_json = serde_json::to_string(&serde_json::json!({
                        "results": dialog.results,
                        "candidates": dialog.candidates,
                    }))
                    .map_err(|_| Status::internal("could not encode match results"))?;
                    self.engine.library_input(Input::CloseMatch);
                    return Ok(Response::new(SearchMatchesReply { results_json }));
                }
                if let Some(error) = &dialog.error {
                    self.engine.library_input(Input::CloseMatch);
                    return Err(Status::unavailable(format!(
                        "match search failed: {error:?}"
                    )));
                }
            } else if matches!(
                snapshot.auth,
                gamenode_core::AuthView::SignedOut | gamenode_core::AuthView::RestoreFailed
            ) {
                return Err(Status::failed_precondition(
                    "sign in before searching GameNode",
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                self.engine.library_input(Input::CloseMatch);
                return Err(Status::deadline_exceeded(
                    "timed out waiting for match search",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    async fn mark_no_game(
        &self,
        request: Request<GameRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        use gamenode_core::library::{Input, SelectMode};
        let key = request.into_inner().entry_key;
        if !self.engine.installation_directories().contains_key(&key) {
            return Err(Status::not_found("installation not found"));
        }
        self.engine.library_input(Input::Select {
            key,
            mode: SelectMode::Replace,
        });
        self.engine
            .library_input(Input::MarkNoGame { now: unix_now() });
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn clear_match(
        &self,
        request: Request<GameRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        use gamenode_core::library::{Input, SelectMode};
        let key = request.into_inner().entry_key;
        if !self.engine.installation_directories().contains_key(&key) {
            return Err(Status::not_found("installation not found"));
        }
        self.engine.library_input(Input::Select {
            key,
            mode: SelectMode::Replace,
        });
        self.engine.library_input(Input::ClearMatch);
        Ok(Response::new(ActionReply { accepted: true }))
    }
    async fn add_game(
        &self,
        request: Request<AddGameRequest>,
    ) -> Result<Response<AddGameReply>, Status> {
        use gamenode_core::library::{Input, SelectMode};
        let request = request.into_inner();
        if (request.status_only
            && (request.clear_collections || !request.collection_ids.is_empty()))
            || (request.clear_collections && !request.collection_ids.is_empty())
        {
            return Err(Status::invalid_argument(
                "games status cannot change collections",
            ));
        }
        if !self
            .engine
            .installation_directories()
            .contains_key(&request.entry_key)
        {
            return Err(Status::not_found("installation not found"));
        }
        let status = match request.status.to_ascii_lowercase().as_str() {
            "playing" => gamenode_core::library::EntryStatus::Playing,
            "finished" => gamenode_core::library::EntryStatus::Finished,
            "planned" => gamenode_core::library::EntryStatus::Planned,
            "dropped" => gamenode_core::library::EntryStatus::Dropped,
            "ongoing" => gamenode_core::library::EntryStatus::Ongoing,
            _ => {
                return Err(Status::invalid_argument(
                    "status must be playing, finished, planned, dropped, or ongoing",
                ));
            }
        };
        self.engine.library_input(Input::Select {
            key: request.entry_key,
            mode: SelectMode::Replace,
        });
        self.engine.library_input(Input::OpenAdd);
        self.engine.library_input(Input::SetAddStatus(status));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let snapshot = self.engine.snapshot();
            if let Some(add) = snapshot
                .library
                .as_ref()
                .and_then(|view| view.add_dialog.as_ref())
            {
                if let Some(summary) = &add.summary {
                    let summary_json = serde_json::to_string(summary)
                        .map_err(|_| Status::internal("could not encode add result"))?;
                    self.engine.library_input(Input::CloseAdd);
                    return Ok(Response::new(AddGameReply { summary_json }));
                }
                if let Some(error) = &add.error {
                    self.engine.library_input(Input::CloseAdd);
                    return Err(Status::unavailable(format!(
                        "could not add game: {error:?}"
                    )));
                }
                if add.can_confirm
                    && matches!(
                        &add.collections,
                        gamenode_core::library::CollectionsView::Loaded(_)
                    )
                {
                    let gamenode_core::library::CollectionsView::Loaded(collections) =
                        &add.collections
                    else {
                        unreachable!()
                    };
                    if request.status_only && !add.editing_existing {
                        self.engine.library_input(Input::CloseAdd);
                        return Err(Status::failed_precondition(
                            "game is not yet in the GameNode library",
                        ));
                    }
                    if request.status_only && !request.collection_ids.is_empty() {
                        self.engine.library_input(Input::CloseAdd);
                        return Err(Status::invalid_argument(
                            "games status cannot change collections",
                        ));
                    }
                    for id in &request.collection_ids {
                        if !collections.iter().any(|collection| collection.id == *id) {
                            self.engine.library_input(Input::CloseAdd);
                            return Err(Status::invalid_argument(format!(
                                "unknown collection id: {id}"
                            )));
                        }
                    }
                    let requested: std::collections::HashSet<_> =
                        request.collection_ids.iter().map(String::as_str).collect();
                    for collection in collections {
                        let should_be_picked = if request.status_only {
                            collection.picked
                        } else if request.clear_collections {
                            false
                        } else if request.collection_ids.is_empty() {
                            collection.picked
                        } else {
                            requested.contains(collection.id.as_str())
                        };
                        if collection.picked != should_be_picked {
                            self.engine
                                .library_input(Input::ToggleAddCollection(collection.id.clone()));
                        }
                    }
                    self.engine.library_input(Input::ConfirmAdd);
                }
            } else if matches!(
                snapshot.auth,
                gamenode_core::AuthView::SignedOut | gamenode_core::AuthView::RestoreFailed
            ) {
                return Err(Status::failed_precondition("sign in before adding a game"));
            }
            if tokio::time::Instant::now() >= deadline {
                self.engine.library_input(Input::CloseAdd);
                return Err(Status::deadline_exceeded(
                    "timed out waiting for GameNode add result",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    async fn stop(&self, _: Request<Empty>) -> Result<Response<StopReply>, Status> {
        if self
            .engine
            .snapshot()
            .library
            .is_some_and(|library| library.scanning)
        {
            return Err(Status::failed_precondition(
                "cannot stop while a library scan is running",
            ));
        }
        self.stop.store(true, Ordering::Release);
        self.engine.flush_storage().await;
        Ok(Response::new(StopReply { accepted: true }))
    }
}

pub async fn serve(
    engine: Engine,
    socket: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(unix))]
    return Err("the first daemon transport currently requires Unix domain sockets".into());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let owner_uid = std::fs::metadata(&socket)?.uid();
        let incoming =
            UnixListenerStream::new(listener).filter_map(move |accepted| match accepted {
                Ok(stream) if stream.peer_cred().is_ok_and(|cred| cred.uid() == owner_uid) => {
                    Some(Ok::<_, std::io::Error>(stream))
                }
                _ => None,
            });
        let stop = Arc::new(AtomicBool::new(false));
        let service = Service::new(engine, stop.clone());
        Server::builder()
            .add_service(ClientServer::new(service))
            .serve_with_incoming_shutdown(incoming, async move {
                while !stop.load(Ordering::Acquire) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            })
            .await?;
        let _ = std::fs::remove_file(socket);
        Ok(())
    }
}

pub async fn connect(
    socket: std::path::PathBuf,
) -> Result<proto::client_client::ClientClient<tonic::transport::Channel>, Box<dyn std::error::Error>>
{
    use hyper_util::rt::TokioIo;
    use std::os::unix::fs::MetadataExt;
    use tower::service_fn;
    let channel = tonic::transport::Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(move |_| {
            let path = socket.clone();
            async move {
                let expected_uid = std::fs::metadata(&path)?.uid();
                let stream = tokio::net::UnixStream::connect(path).await?;
                if !stream
                    .peer_cred()
                    .is_ok_and(|cred| cred.uid() == expected_uid)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "daemon peer uid mismatch",
                    ));
                }
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await?;
    Ok(proto::client_client::ClientClient::new(channel))
}
