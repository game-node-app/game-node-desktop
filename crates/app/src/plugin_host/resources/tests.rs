//! ResourceHost library reads over the real local socket, with the SDK as the client.

use super::*;
use crate::plugin_host::files::FileAccess;
use crate::plugin_host::library::{
    CatalogGame, CommitGuard, FavoriteWrite, InstallState, LibraryHost, LibrarySource, MatchWrite,
    StoredArtwork,
};
use crate::plugin_host::network::NetworkAccess;
use crate::plugin_host::notifications::Notifications;
use crate::plugin_host::provider::{HostServices, Lease, Registration, RegistrationOrigin};
use crate::plugin_host::storage::PluginStorage;
use gamenode_core::library::{ArtworkWrite, DisplayWrite};
use gamenode_platform::plugin_artwork::Prepared;
use gamenode_platform::plugin_files::Reserved;
use gamenode_plugin_sdk::{HostClient, Launch, Session, SessionOptions};
use resource_host_client::ResourceHostClient;
use resource_host_server::ResourceHost;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tonic::Code;
use tonic::transport::Channel;

mod artwork_handles;
mod backups;
mod data;
mod declared_locations;
mod diagnostics;
mod files;
mod fingerprints;
mod games;
mod jobs;
mod libraries;
mod locations;
mod network;
mod notifications;
mod observations;
mod performance;
mod persistence;
mod ports;
mod runtimes;
mod saves;
mod sdk;
mod sdk_actions;
mod sdk_utilities;
mod storage;
mod tools;

#[path = "../../../../plugin-sdk/examples/action_recipes.rs"]
mod action_recipes;

struct NoServices;

#[tonic::async_trait]
impl HostServices for NoServices {
    async fn context(&self, _: Lease) -> Result<ProviderContext, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn account(&self, _: Lease, _: account_request::Action) -> Result<AccountReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn secret(&self, _: Lease, _: SecretRequest) -> Result<SecretReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn login(&self, _: Lease, _: LoginRequest) -> Result<LoginReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn actions(&self, _: Lease, _: ActionDeclaration) -> Result<ActionReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

struct Fixture {
    api: ProviderApi,
    feed: Arc<LibraryFeed>,
    storage: Arc<PluginStorage>,
    files: Arc<FileAccess>,
    network: Arc<NetworkAccess>,
    fetcher: Arc<network::FakeFetcher>,
    notifications: Arc<Notifications>,
    /// The jobs plugins started; tests start some from the host side.
    jobs: Jobs,
    /// The intents and the dialog behind game launches and installs.
    intents: Arc<Intents>,
    /// The grants for other plugins' save profiles.
    saves: Arc<crate::plugin_host::saves::SaveAccess>,
    /// The fingerprints of approved places.
    snapshots: Arc<crate::plugin_host::fingerprints::Snapshots>,
    /// Shared observations inserted by source fixtures for cross-plugin read tests.
    data_broker: Arc<crate::plugin_host::shared_data::DataBroker>,
    /// Host-owned cross-plugin consent store used by producer/consumer fixtures.
    shared_policies: Arc<crate::plugin_host::sharing_policy::SharedSharingPolicies>,
    /// The time the notification rate limit sees; tests move it.
    notice_clock: Arc<std::sync::atomic::AtomicU64>,
    notices: Arc<notifications::Collected>,
    /// The seconds the storage clock reads; tests move it to expire values.
    clock: Arc<std::sync::atomic::AtomicI64>,
    plugin_data: std::path::PathBuf,
    endpoint: std::path::PathBuf,
    capabilities: Vec<&'static str>,
    host: HostClient,
    _commands: tonic::Streaming<ProviderCommand>,
    _server: crate::plugin_host::PluginApiServer,
    _temp: tempfile::TempDir,
}

impl Fixture {
    /// A third-party provider `fixture` signed in as account `a`, negotiating API 1.6.
    async fn new(grants: &[&'static str]) -> Self {
        Self::build(grants, true).await
    }

    async fn build(grants: &[&'static str], api_1_6: bool) -> Self {
        Self::start(grants, api_1_6, None).await
    }

    /// Like [`new`](Self::new) over the plugin data folder of an earlier fixture, as after a
    /// restart of the host: durable values are still there, session-only ones are not.
    async fn restarted(grants: &[&'static str], plugin_data: std::path::PathBuf) -> Self {
        Self::start(grants, true, Some(plugin_data)).await
    }

    async fn start(
        grants: &[&'static str],
        api_1_6: bool,
        plugin_data: Option<std::path::PathBuf>,
    ) -> Self {
        Self::start_with_target(grants, api_1_6, plugin_data, None).await
    }

    async fn start_with_target(
        grants: &[&'static str],
        api_1_6: bool,
        plugin_data: Option<std::path::PathBuf>,
        target: Option<TargetGameContext>,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = temp.path().join("plugins.sock");
        let plugin_data = plugin_data.unwrap_or_else(|| temp.path().join("plugin-data"));
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(1_000_000));
        let storage = Arc::new(PluginStorage::new(&plugin_data).with_clock({
            let clock = clock.clone();
            move || clock.load(Ordering::SeqCst)
        }));
        // The host's data folder is reserved: no file handle reaches it.
        std::fs::create_dir_all(&plugin_data).unwrap();
        let files = Arc::new(FileAccess::new(Reserved::new(std::slice::from_ref(
            &plugin_data,
        ))));
        // Fetches go to a scripted stand-in; the origins plugins may reach start empty.
        let fetcher = Arc::new(network::FakeFetcher::default());
        let network = Arc::new(NetworkAccess::new(fetcher.clone()));
        // Notifications go to a collector; the rate limit follows a clock the tests move.
        let notice_base = std::time::Instant::now();
        let notice_clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let notifications = Arc::new({
            let clock = notice_clock.clone();
            Notifications::default().with_clock(move || {
                notice_base + Duration::from_millis(clock.load(Ordering::SeqCst))
            })
        });
        let notices = Arc::new(notifications::Collected::default());
        notifications.set_sink(notices.clone());
        let data_broker = Arc::new(crate::plugin_host::shared_data::DataBroker::new());
        let root = crate::plugin_host::PluginApi {
            storage: storage.clone(),
            files: files.clone(),
            network: network.clone(),
            notifications: notifications.clone(),
            data_broker: data_broker.clone(),
            ..Default::default()
        };
        let shared_policies = root.shared_policies.clone();
        root.set_state_layout(crate::plugin_host::profile::PluginStateLayout::new(
            &plugin_data,
        ));
        let jobs = root.jobs.clone();
        let intents = root.intents.clone();
        let saves = root.saves.clone();
        let snapshots = root.fingerprints.clone();
        let api = root.providers.clone();
        let feed = root.library.clone();
        api.profiles().update(Some("a"), true);
        let extension_session = target.is_some();
        let mut capabilities = vec![cap::CONTEXT];
        capabilities.extend_from_slice(grants);
        let registration_id = if extension_session {
            crate::plugin_host::provider::extension_registration_id("fixture.plugin")
        } else {
            "fixture".into()
        };
        api.register(Registration {
            declared_developer: "Fixture developer".into(),
            package_digest: vec![7; 32],
            provider_id: registration_id.clone(),
            plugin_id: "fixture.plugin".into(),
            kind: if extension_session {
                crate::plugin_host::provider::RegistrationKind::Extension
            } else {
                crate::plugin_host::provider::RegistrationKind::Provider
            },
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "ab".repeat(32),
            },
            capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(NoServices),
        })
        .unwrap();
        let launch = match &target {
            Some(target) => {
                api.begin_extension_launch(&registration_id, api.profiles().epoch(), target.clone())
            }
            None => api.begin_launch(&registration_id),
        }
        .unwrap();
        let credential = launch
            .credential
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect();
        let server = crate::plugin_host::PluginApiServer::start(
            &tokio::runtime::Handle::current(),
            &endpoint,
            root,
        )
        .unwrap();
        let client_launch = Launch {
            endpoint: endpoint.clone(),
            credential,
            operation_id: "launch".into(),
        };
        let options = if api_1_6 {
            SessionOptions {
                min_api_major: 1,
                min_api_minor: cap::THIRD_PARTY_API_MINOR,
                max_api_major: 1,
                max_api_minor: gamenode_plugin_protocol::API_MINOR,
                session_kind: if extension_session {
                    SessionKind::Extension
                } else {
                    SessionKind::Unspecified
                },
                target_game: target,
                ..SessionOptions::default()
            }
        } else {
            SessionOptions::default()
        };
        let session = Session::open_with_options(
            client_launch,
            "fixture.plugin",
            if extension_session { "" } else { "fixture" },
            "1.0",
            &capabilities,
            options,
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        Self {
            api,
            feed,
            storage,
            files,
            network,
            fetcher,
            notifications,
            jobs,
            intents,
            saves,
            snapshots,
            notice_clock,
            notices,
            data_broker,
            shared_policies,
            clock,
            plugin_data,
            endpoint,
            capabilities: capabilities.clone(),
            host: session.host,
            _commands: session.commands,
            _server: server,
            _temp: temp,
        }
    }

    fn client(&self) -> ResourceHostClient<Channel> {
        self.host.resource_client()
    }

    fn context(&self) -> ServiceContext {
        self.host.resource_context().unwrap().clone()
    }

    /// Syncs library rows and gives each the id `id-<key>`.
    fn library(&self, rows: &[(&str, Option<u64>, &str)]) {
        let missing = self.feed.sync(
            rows.iter()
                .map(|(key, game_id, name)| LibrarySource {
                    entry_key: (*key).into(),
                    provider_id: key.split(':').next().unwrap().into(),
                    game_id: *game_id,
                    name: (*name).into(),
                    install_state: InstallState::Installed,
                    display_name: None,
                    metadata_revision: 0,
                    artwork_revision: 0,
                    install_dir: String::new(),
                    detected_target: None,
                })
                .collect(),
        );
        let ids = missing
            .iter()
            .map(|key| (key.clone(), format!("id-{key}")))
            .collect();
        self.feed.assign_ids(&missing, ids);
    }

    fn list_request(&self, page_size: u32, cursor: &str, provider_id: &str) -> LibraryListRequest {
        LibraryListRequest {
            page: Some(PageRequest {
                context: Some(self.context()),
                cursor: cursor.into(),
                page_size,
                common_data_categories: None,
            }),
            provider_id: provider_id.into(),
            search: String::new(),
        }
    }

    async fn list(&self, provider_id: &str) -> Result<LibraryListReply, Status> {
        self.client()
            .library_list(self.list_request(0, "", provider_id))
            .await
            .map(Response::into_inner)
    }

    async fn get(&self, installation_id: &str, game_id: &str) -> Result<LibraryEntry, Status> {
        self.client()
            .library_get(LibraryGetRequest {
                context: Some(self.context()),
                game_id: game_id.into(),
                installation_id: installation_id.into(),
                common_data_categories: None,
            })
            .await
            .map(|reply| reply.into_inner().entry.unwrap())
    }

    async fn watch(&self, after_revision: u64) -> Result<tonic::Streaming<LibraryEvent>, Status> {
        self.client()
            .library_watch(LibraryWatchRequest {
                context: Some(self.context()),
                after_revision,
                provider_id: String::new(),
                common_data_categories: None,
            })
            .await
            .map(Response::into_inner)
    }
}

fn names(reply: &LibraryListReply) -> Vec<&str> {
    reply.entries.iter().map(|e| e.name.as_str()).collect()
}

async fn next(stream: &mut tonic::Streaming<LibraryEvent>) -> Result<LibraryEvent, Status> {
    tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("an event in time")
        .map(|event| event.expect("the stream is open"))
}

const SHARED: &[(&str, Option<u64>, &str)] = &[
    ("fixture:1", Some(7), "Portal 2"),
    ("fixture:2", None, "Homebrew"),
    ("steam:620", Some(7), "Portal 2"),
    ("steam:570", None, "Dota 2"),
];

#[tokio::test]
async fn own_provider_reads_never_disclose_another_providers_entries() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);

    let reply = fixture.list("").await.unwrap();
    assert_eq!(names(&reply), ["Portal 2", "Homebrew"]);
    assert!(reply.entries.iter().all(|e| e.provider_id == "fixture"));
    assert_eq!(reply.revision, 1);
    // Naming the other provider does not widen the scope.
    assert!(fixture.list("steam").await.unwrap().entries.is_empty());
    // A foreign installation id is indistinguishable from an unknown one.
    for id in ["id-steam:620", "id-nope"] {
        let status = fixture.get(id, "").await.unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{id}");
        assert_eq!(status.message(), "library entry not found");
    }
    // Asking by game finds only the caller's own installation of it.
    let own = fixture.get("", "7").await.unwrap();
    assert_eq!(own.installation_id, "id-fixture:1");
}

#[tokio::test]
async fn the_whole_library_needs_its_own_grant() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ, cap::LIBRARY_READ_ALL]).await;
    fixture.library(SHARED);

    assert_eq!(fixture.list("").await.unwrap().entries.len(), 4);
    let steam = fixture.list("steam").await.unwrap();
    assert_eq!(names(&steam), ["Dota 2", "Portal 2"]);
    assert_eq!(
        fixture.get("id-steam:570", "").await.unwrap().name,
        "Dota 2"
    );
    // Two installations of game 7 are visible now, so a game id alone is ambiguous.
    let status = fixture.get("", "7").await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn game_and_installation_identities_stay_separate() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);

    // Unmatched: empty game id, stable installation id.
    let homebrew = fixture.get("id-fixture:2", "").await.unwrap();
    assert_eq!(homebrew.game_id, "");
    assert_eq!(homebrew.install_state, "installed");
    // Naming both requires them to agree.
    assert_eq!(
        fixture.get("id-fixture:1", "8").await.unwrap_err().code(),
        Code::NotFound
    );
    assert_eq!(fixture.get("id-fixture:1", "7").await.unwrap().game_id, "7");
    assert_eq!(
        fixture.get("", "seven").await.unwrap_err().code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture.get("", "").await.unwrap_err().code(),
        Code::InvalidArgument
    );

    // A rescan that matches the homebrew entry keeps its installation id.
    fixture.library(&[
        ("fixture:1", Some(7), "Portal 2"),
        ("fixture:2", Some(99), "Homebrew"),
    ]);
    let matched = fixture.get("", "99").await.unwrap();
    assert_eq!(matched.installation_id, homebrew.installation_id);
    assert!(matched.revision > homebrew.revision);
}

#[tokio::test]
async fn pages_are_bounded_and_a_stale_cursor_must_restart_the_list() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let rows: Vec<(String, Option<u64>, String)> = (0..250)
        .map(|i| (format!("fixture:{i:03}"), None, format!("Game {i:03}")))
        .collect();
    let borrowed: Vec<(&str, Option<u64>, &str)> = rows
        .iter()
        .map(|(k, g, n)| (k.as_str(), *g, n.as_str()))
        .collect();
    fixture.library(&borrowed);
    let mut client = fixture.client();

    let first = client
        .library_list(fixture.list_request(1000, "", ""))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.entries.len(), MAX_PAGE_SIZE, "clamped to the maximum");
    let rest = client
        .library_list(fixture.list_request(1000, &first.next_cursor, ""))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(rest.entries.len(), 50);
    assert!(rest.next_cursor.is_empty());
    assert!(first.entries.last().unwrap().installation_id < rest.entries[0].installation_id);
    let default = client
        .library_list(fixture.list_request(0, "", ""))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(default.entries.len(), DEFAULT_PAGE_SIZE);

    // The cursor belongs to its query: another provider filter cannot reuse it.
    let status = client
        .library_list(fixture.list_request(10, &default.next_cursor, "fixture"))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    // A forged cursor is invalid, not stale.
    let status = client
        .library_list(fixture.list_request(10, "00.00", ""))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);

    // The library changes: the next page is refused without entries.
    fixture.library(&borrowed[..249]);
    let status = client
        .library_list(fixture.list_request(10, &default.next_cursor, ""))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(status.message().contains("restart the list"));
}

#[tokio::test]
async fn common_data_projections_are_refused_rather_than_narrowed() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let mut request = fixture.list_request(10, "", "");
    request.page.as_mut().unwrap().common_data_categories = Some(CommonDataSelector {
        categories: vec![CommonDataCategory::GamePaths as i32],
        selected_record_ids: vec![],
    });
    let status = fixture.client().library_list(request).await.unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    // An empty selector is the default private query.
    let mut request = fixture.list_request(10, "", "");
    request.page.as_mut().unwrap().common_data_categories = Some(CommonDataSelector::default());
    assert!(fixture.client().library_list(request).await.is_ok());
}

#[tokio::test]
async fn only_the_sessions_resource_context_with_a_grant_and_a_profile_is_accepted() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    let mut client = fixture.client();
    let attempt = |context: Option<ServiceContext>| LibraryGetRequest {
        context,
        game_id: String::new(),
        installation_id: "id-fixture:1".into(),
        common_data_categories: None,
    };

    let status = client.library_get(attempt(None)).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    // The launch context is not a resource context.
    let status = client
        .library_get(attempt(Some(fixture.host.context().clone())))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let mut forged = fixture.context();
    forged.operation_id = "guessed".into();
    let status = client.library_get(attempt(Some(forged))).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let mut wrong = fixture.context();
    wrong.session_credential = vec![0; 32];
    let status = client.library_get(attempt(Some(wrong))).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let mut other_provider = fixture.context();
    other_provider.provider_id = "steam".into();
    let status = client
        .library_get(attempt(Some(other_provider)))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);

    // Without a signed-in account the service is unavailable, not denied.
    fixture.api.profiles().update(None, false);
    let status = client
        .library_get(attempt(Some(fixture.context())))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    // Another account is another profile: the old context is stale.
    fixture.api.profiles().update(Some("b"), true);
    let status = client
        .library_get(attempt(Some(fixture.context())))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn library_reads_require_the_library_grant() {
    let fixture = Fixture::new(&[]).await;
    fixture.library(SHARED);
    let status = fixture.list("").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let audit = fixture.api.authorization_audit();
    assert!(
        audit
            .iter()
            .any(|event| event.permission_id == cap::LIBRARY_READ)
    );
}

#[tokio::test]
async fn sessions_before_api_1_6_get_no_resource_context() {
    let fixture = Fixture::build(&[], false).await;
    assert!(fixture.host.resource_context().is_none());
}

#[tokio::test]
async fn watch_resumes_filters_by_scope_and_ends_on_revocation() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(&[("fixture:1", None, "A")]);
    fixture.library(&[("fixture:1", Some(3), "A"), ("steam:1", None, "S")]);

    // Resume after revision 1: the retained change to the own entry, not the foreign one.
    let mut stream = fixture.watch(1).await.unwrap();
    let event = next(&mut stream).await.unwrap();
    assert_eq!((event.revision, event.event_type.as_str()), (2, "upserted"));
    assert_eq!(event.entry.as_ref().unwrap().game_id, "3");

    // Live: a foreign change is skipped, the own removal arrives.
    fixture.library(&[("fixture:1", Some(3), "A"), ("steam:1", None, "S2")]);
    fixture.library(&[("steam:1", None, "S2")]);
    // Revision 3 was the foreign entry getting its id, 4 its rename, 5 the own removal.
    let event = next(&mut stream).await.unwrap();
    assert_eq!((event.revision, event.event_type.as_str()), (5, "removed"));
    assert_eq!(
        event.entry.as_ref().unwrap().installation_id,
        "id-fixture:1"
    );

    // Revoking the grant ends the stream with a denial.
    fixture
        .api
        .revoke_session_permission("fixture", cap::LIBRARY_READ)
        .await;
    fixture.library(&[("fixture:9", None, "Z")]);
    let status = next(&mut stream).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn watch_positions_outside_the_history_must_resync() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    for i in 0..(crate::plugin_host::library::HISTORY_EVENTS as u64 + 2) {
        fixture.library(&[("fixture:1", Some(i + 1), "A")]);
    }
    let status = fixture.watch(1).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let status = fixture.watch(10_000).await.unwrap_err();
    assert_eq!(status.code(), Code::OutOfRange);
    // Zero starts at the current revision with no backlog.
    let mut stream = fixture.watch(0).await.unwrap();
    fixture.library(&[("fixture:1", None, "A")]);
    assert_eq!(next(&mut stream).await.unwrap().entry.unwrap().game_id, "");
}

#[tokio::test]
async fn a_session_may_hold_only_a_bounded_number_of_watches() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let service = ResourceApi::new(
        fixture.api.clone(),
        fixture.feed.clone(),
        fixture.storage.clone(),
        fixture.files.clone(),
        fixture.network.clone(),
        fixture.notifications.clone(),
        fixture.jobs.clone(),
        fixture.intents.clone(),
        fixture.saves.clone(),
        fixture.snapshots.clone(),
    );
    let request = || {
        Request::new(LibraryWatchRequest {
            context: Some(fixture.context()),
            after_revision: 0,
            provider_id: String::new(),
            common_data_categories: None,
        })
    };
    let mut open = Vec::new();
    for _ in 0..MAX_WATCHES_PER_SESSION {
        open.push(service.library_watch(request()).await.unwrap());
    }
    let status = service.library_watch(request()).await.err().unwrap();
    assert_eq!(status.code(), Code::ResourceExhausted);
    // Closing one frees its slot once its task notices.
    drop(open.pop());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match service.library_watch(request()).await {
            Ok(_) => break,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            Err(status) => panic!("{status:?}"),
        }
    }
}

#[tokio::test]
async fn custom_launch_without_its_own_grant_is_denied_and_describe_lists_the_routes() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let status = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let status = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            recipe_id: "recipe-without-grant".into(),
            recipe_revision: 1,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let described = fixture.host.describe_api().await.unwrap();
    let resource: Vec<_> = described
        .methods
        .iter()
        .filter(|method| method.service == "ResourceHost")
        .map(|method| (method.method.as_str(), method.max_page_size))
        .collect();
    assert_eq!(
        resource,
        [
            ("PortsList", MAX_PAGE_SIZE as u32),
            ("PortsDetail", 0),
            ("PortsRequestInstall", 0),
            ("ToolsList", MAX_PAGE_SIZE as u32),
            ("ToolsRequestApply", 0),
            ("ToolsRequestRemove", 0),
            ("BackupsList", MAX_PAGE_SIZE as u32),
            ("BackupsRequestCreate", 0),
            ("BackupsRequestRestore", 0),
            ("LibrariesList", MAX_PAGE_SIZE as u32),
            ("LibrariesGet", 0),
            ("LibrariesWatch", 0),
            ("LibrariesCreate", 0),
            ("LibrariesUpdate", 0),
            ("LibrariesRemove", 0),
            ("LibrariesAddRoot", 0),
            ("LibrariesUpdateRoot", 0),
            ("LibrariesRemoveRoot", 0),
            ("LibraryList", MAX_PAGE_SIZE as u32),
            ("LibraryGet", 0),
            ("LibraryWatch", 0),
            ("RuntimesList", MAX_PAGE_SIZE as u32),
            ("RuntimesGet", 0),
            ("RuntimesWatch", 0),
            ("RuntimesRegister", 0),
            ("RuntimesUpdate", 0),
            ("RuntimesRemove", 0),
            ("GamesRegisterContent", 0),
            ("MetadataUpdate", 0),
            ("FavoritesList", MAX_PAGE_SIZE as u32),
            ("FavoritesSet", 0),
            ("MatchingSearch", 0),
            ("MatchingAssign", 0),
            ("ArtworkRead", 0),
            ("ArtworkSet", 0),
            ("StorageGet", 0),
            ("StoragePut", 0),
            ("StorageDelete", 0),
            ("FilesRequestAccess", 0),
            ("FilesList", MAX_PAGE_SIZE as u32),
            ("FilesRead", 0),
            ("FilesWrite", 0),
            ("FilesDelete", 0),
            ("NetworkFetch", 0),
            ("NotificationsPublish", 0),
            ("GamesRequestLaunch", 0),
            ("GamesProposeCustomLaunch", 0),
            ("GamesRequestCustomLaunch", 0),
            ("GamesRemoveCustomLaunch", 0),
            ("GamesRequestInstall", 0),
            ("JobsGet", 0),
            ("JobsWatch", 0),
            ("JobsCancel", 0),
        ]
    );
    // A job needs the permission of the operation that made it, so the jobs methods name none.
    assert!(
        described
            .methods
            .iter()
            .filter(|method| method.method.starts_with("Jobs"))
            .all(|method| method.permissions.is_empty())
    );
}

// ---- Display metadata and favorites (task 5.2) ----

/// An entry's explicit match: the game, whether the user made it, and the entry revision.
type FakeMatch = (Option<u64>, bool, u64);

/// An in-memory engine side: display overrides by entry key and favorite state by
/// `(provider, game)`. A game is "matched" for a provider when it has a row here.
#[derive(Default)]
struct FakeHost {
    port_support: AtomicBool,
    port_records: Mutex<Vec<PortRecord>>,
    port_reviews: Mutex<Vec<gamenode_core::ports::view::PortPluginRequest>>,
    port_gate: tokio::sync::Notify,
    port_effects: AtomicUsize,
    port_declined: AtomicBool,
    tool_support: AtomicBool,
    tool_reviews: Mutex<Vec<(String, String, bool)>>,
    tool_gate: tokio::sync::Notify,
    tool_effects: AtomicUsize,
    tool_declined: AtomicBool,
    backup: Mutex<Option<crate::plugin_host::library::BackupHostContext>>,
    scan_roots: Mutex<HashMap<String, Vec<crate::plugin_host::library::PluginScanRoot>>>,
    names: Mutex<HashMap<String, (Option<String>, u64)>>,
    games: Mutex<BTreeMap<(String, u64), bool>>,
    /// State-changing writes that reached the host.
    writes: AtomicUsize,
    /// Revokes a permission right before the commit guard runs, as a race would.
    revoke_before_commit: Mutex<Option<(ProviderApi, &'static str)>>,
    /// What the server's catalog knows, in answer order.
    catalog: Mutex<Vec<CatalogGame>>,
    catalog_down: AtomicBool,
    searches: AtomicUsize,
    /// Games the device knows without a search, and games a search offered.
    known: Mutex<std::collections::HashSet<u64>>,
    offered: Mutex<std::collections::HashSet<u64>>,
    /// Explicit matches by entry key: game, whether the user made it, entry revision.
    matches: Mutex<HashMap<String, FakeMatch>>,
    /// Cover overrides by entry key: the stored picture (`None` once cleared) and its revision.
    covers: Mutex<HashMap<String, (Option<Prepared>, u64)>>,
    /// The covers of the games entries are matched to, by entry key.
    game_covers: Mutex<HashMap<String, Vec<u8>>>,
    /// Entries the library was asked to open: the entry key and whether it was an install.
    opened: Mutex<Vec<(String, bool)>>,
    /// The code the next opening fails with, if any.
    open_fails: Mutex<Option<tonic::Code>>,
    /// What a launch would start now; tests change it to change the target.
    launch_target: std::sync::atomic::AtomicU64,
    custom_launch_recipes:
        Mutex<HashMap<(String, String), gamenode_core::library::CustomLaunchRecipe>>,
    launch_recipe_selections: Mutex<HashMap<String, gamenode_core::library::LaunchRecipeSelection>>,
    custom_launch_dispatches: Mutex<Vec<crate::plugin_host::library::CustomLaunchDispatch>>,
}

impl FakeHost {
    fn hook(&self) -> Option<(ProviderApi, &'static str)> {
        self.revoke_before_commit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn name_of(&self, key: &str) -> Option<(Option<String>, u64)> {
        self.names.lock().unwrap().get(key).cloned()
    }
}

#[tonic::async_trait]
impl LibraryHost for FakeHost {
    fn ports_supported(&self) -> bool {
        self.port_support.load(Ordering::SeqCst)
    }
    async fn ports_catalog(&self, detail: Option<String>) -> Result<Vec<PortRecord>, Status> {
        let records = self
            .port_records
            .lock()
            .unwrap()
            .iter()
            .filter(|p| detail.as_ref().is_none_or(|id| *id == p.port_id))
            .cloned()
            .collect::<Vec<_>>();
        if detail.is_some() && records.is_empty() {
            return Err(Status::not_found("port not found"));
        }
        Ok(records)
    }
    async fn request_port(
        &self,
        port: String,
        review: gamenode_core::ports::view::PortPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), Status> {
        if !self
            .port_records
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.port_id == port)
        {
            return Err(Status::not_found("port not found"));
        }
        let check_before = check.clone();
        tokio::task::spawn_blocking(move || check_before())
            .await
            .unwrap()?;
        self.port_reviews.lock().unwrap().push(review);
        self.port_gate.notified().await;
        tokio::task::spawn_blocking(move || check())
            .await
            .unwrap()?;
        if self.port_declined.load(Ordering::SeqCst) {
            return Err(Status::cancelled("declined"));
        }
        self.port_effects.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn tools_supported(&self) -> bool {
        self.tool_support.load(Ordering::SeqCst)
    }
    async fn tools_list(&self, _: String) -> Result<Vec<ToolRecord>, Status> {
        Ok(gamenode_core::tools::builtin_tools()
            .into_iter()
            .map(|d| ToolRecord {
                tool_id: d.id.as_str().into(),
                name: d.name,
                version: "fixture-v1".into(),
                availability: if self.tools_supported() {
                    "available"
                } else {
                    "unavailable"
                }
                .into(),
            })
            .collect())
    }
    async fn request_tool(
        &self,
        key: String,
        tool: String,
        review: gamenode_core::tools::ToolPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), Status> {
        check()?;
        self.tool_reviews
            .lock()
            .unwrap()
            .push((key, tool, review.remove));
        self.tool_gate.notified().await;
        check()?;
        if self.tool_declined.load(Ordering::SeqCst) {
            return Err(Status::cancelled("declined"));
        }
        self.tool_effects.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn backup_context(
        &self,
        _: String,
    ) -> Result<crate::plugin_host::library::BackupHostContext, Status> {
        self.backup
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Status::unavailable("backup engine unavailable"))
    }
    fn plugin_roots_supported(&self) -> bool {
        true
    }
    fn set_plugin_roots(
        &self,
        owner: String,
        roots: Vec<crate::plugin_host::library::PluginScanRoot>,
    ) -> Result<(), Status> {
        self.scan_roots.lock().unwrap().insert(owner, roots);
        Ok(())
    }

    async fn open_entry(&self, entry_key: String, install: bool) -> Result<(), Status> {
        if let Some(code) = *self.open_fails.lock().unwrap() {
            return Err(Status::new(code, "the provider could not do that"));
        }
        self.opened.lock().unwrap().push((entry_key, install));
        Ok(())
    }

    async fn launch_fingerprint(&self, _entry_key: String) -> Result<u64, Status> {
        Ok(self.launch_target.load(Ordering::SeqCst))
    }

    async fn launch_target_revision(&self, _entry_key: String) -> Result<u64, Status> {
        Ok(1)
    }

    async fn custom_launch_recipe(
        &self,
        owner_key: String,
        recipe_id: String,
    ) -> Result<Option<gamenode_core::library::CustomLaunchRecipe>, Status> {
        Ok(self
            .custom_launch_recipes
            .lock()
            .unwrap()
            .get(&(owner_key, recipe_id))
            .cloned())
    }

    async fn save_custom_launch_recipe(
        &self,
        owner_key: String,
        recipe: gamenode_core::library::CustomLaunchRecipe,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, Status> {
        use gamenode_core::library::CustomLaunchMutation;
        let mut recipes = self.custom_launch_recipes.lock().unwrap();
        let key = (owner_key, recipe.recipe_id.clone());
        if let Some(current) = recipes.get(&key) {
            return Ok(CustomLaunchMutation::Conflict {
                current: current.revision,
            });
        }
        let revision = recipe.revision;
        recipes.insert(key, recipe);
        Ok(CustomLaunchMutation::Applied { revision })
    }

    async fn remove_custom_launch_recipe(
        &self,
        owner_key: String,
        recipe_id: String,
        expected_revision: u64,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, Status> {
        use gamenode_core::library::CustomLaunchMutation;
        let mut recipes = self.custom_launch_recipes.lock().unwrap();
        let key = (owner_key, recipe_id);
        let Some(current) = recipes.get(&key) else {
            return Ok(CustomLaunchMutation::Missing);
        };
        if current.revision != expected_revision {
            return Ok(CustomLaunchMutation::Conflict {
                current: current.revision,
            });
        }
        let revision = current.revision.saturating_add(1);
        recipes.remove(&key);
        Ok(CustomLaunchMutation::Applied { revision })
    }

    async fn launch_recipe_selection(
        &self,
        entry_key: String,
    ) -> Result<Option<gamenode_core::library::LaunchRecipeSelection>, Status> {
        Ok(self
            .launch_recipe_selections
            .lock()
            .unwrap()
            .get(&entry_key)
            .cloned())
    }

    async fn approve_launch_recipe_plan(
        &self,
        entry_key: String,
        recipe_id: String,
        recipe_revision: u64,
        target_revision: u64,
        fingerprint: String,
    ) -> Result<gamenode_core::library::LaunchRecipeSelectionWrite, Status> {
        use gamenode_core::library::LaunchRecipeSelectionWrite;
        let mut selections = self.launch_recipe_selections.lock().unwrap();
        let Some(selection) = selections.get_mut(&entry_key) else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        if selection.recipe_id != recipe_id
            || selection.recipe_revision != recipe_revision
            || selection.base_target_revision != target_revision
        {
            return Ok(LaunchRecipeSelectionWrite::Conflict);
        }
        selection.approved_plan_fingerprint = fingerprint;
        Ok(LaunchRecipeSelectionWrite::Applied)
    }

    async fn prepare_custom_launch(
        &self,
        recipe: gamenode_core::library::CustomLaunchRecipe,
        _executable_path: Option<String>,
        working_directory_path: Option<String>,
        _runtime: Option<crate::plugin_host::library::ResolvedRuntimeLaunch>,
    ) -> Result<crate::plugin_host::library::CustomLaunchDispatch, Status> {
        use crate::plugin_host::library::CustomLaunchDispatch;
        if !recipe.uri.is_empty() {
            let environment = recipe.environment();
            return Ok(CustomLaunchDispatch::Uri {
                uri: url::Url::parse(&recipe.uri)
                    .map_err(|_| Status::invalid_argument("invalid launch URI"))?,
                arguments: recipe.arguments,
                working_directory: working_directory_path.unwrap_or_default(),
                environment,
            });
        }
        Err(Status::unimplemented("fixture supports URI recipes only"))
    }

    async fn launch_custom(
        &self,
        dispatch: crate::plugin_host::library::CustomLaunchDispatch,
    ) -> Result<(), Status> {
        self.custom_launch_dispatches.lock().unwrap().push(dispatch);
        Ok(())
    }

    fn favorites(&self, provider_id: &str) -> Vec<(u64, bool)> {
        self.games
            .lock()
            .unwrap()
            .iter()
            .filter(|((provider, _), _)| provider == provider_id)
            .map(|((_, game), favorite)| (*game, *favorite))
            .collect()
    }

    fn set_favorite(
        &self,
        provider_id: &str,
        game_id: u64,
        favorite: bool,
        guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<FavoriteWrite, Status> {
        if let Some((api, permission)) = self.hook() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(api.revoke_session_permission("fixture", permission))
            });
        }
        guard()?;
        let mut games = self.games.lock().unwrap();
        let Some(current) = games.get_mut(&(provider_id.to_owned(), game_id)) else {
            return Ok(FavoriteWrite::NoMatchedEntry);
        };
        if *current == favorite {
            return Ok(FavoriteWrite::AlreadyApplied);
        }
        *current = favorite;
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(FavoriteWrite::Applied)
    }

    async fn search_catalog(
        &self,
        query: String,
        limit: usize,
    ) -> Result<Vec<CatalogGame>, Status> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        if self.catalog_down.load(Ordering::SeqCst) {
            return Err(Status::unavailable("the GameNode catalog could not answer"));
        }
        let found: Vec<_> = self
            .catalog
            .lock()
            .unwrap()
            .iter()
            .filter(|game| game.name.to_lowercase().contains(&query.to_lowercase()))
            .take(limit)
            .cloned()
            .collect();
        self.offered
            .lock()
            .unwrap()
            .extend(found.iter().map(|game| game.id));
        Ok(found)
    }

    fn assign_match(
        &self,
        entry_key: &str,
        game_id: u64,
        expected_revision: u64,
        guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<MatchWrite, Status> {
        if let Some((api, permission)) = self.hook() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(api.revoke_session_permission("fixture", permission))
            });
        }
        guard()?;
        let mut matches = self.matches.lock().unwrap();
        let (game, by_user, revision) = matches.get(entry_key).copied().unwrap_or((None, false, 1));
        if game == Some(game_id) {
            return Ok(MatchWrite::AlreadyApplied { revision });
        }
        if expected_revision != revision {
            return Ok(MatchWrite::Conflict { current: revision });
        }
        if !self.known.lock().unwrap().contains(&game_id)
            && !self.offered.lock().unwrap().contains(&game_id)
        {
            return Ok(MatchWrite::UnknownGame);
        }
        if by_user {
            return Ok(MatchWrite::UserChoice);
        }
        matches.insert(entry_key.to_owned(), (Some(game_id), false, revision + 1));
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(MatchWrite::Applied {
            revision: revision + 1,
        })
    }

    async fn read_artwork(
        &self,
        entry_key: String,
        max_bytes: u64,
    ) -> Result<StoredArtwork, Status> {
        let covers = self.covers.lock().unwrap();
        let (bytes, media_type, revision) = match covers.get(&entry_key) {
            Some((Some(image), revision)) => (image.bytes.clone(), image.media_type, *revision),
            other => {
                let revision = other.map_or(0, |(_, revision)| *revision);
                let game = self.game_covers.lock().unwrap().get(&entry_key).cloned();
                (
                    game.ok_or_else(|| Status::not_found("the entry has no cover"))?,
                    "image/jpeg",
                    revision,
                )
            }
        };
        if bytes.len() as u64 > max_bytes {
            return Err(Status::resource_exhausted(
                "the cover is larger than max_bytes",
            ));
        }
        Ok(StoredArtwork {
            bytes,
            media_type,
            revision,
        })
    }

    async fn set_artwork(
        &self,
        entry_key: String,
        expected_revision: u64,
        image: Option<Prepared>,
        guard: CommitGuard,
    ) -> Result<ArtworkWrite, Status> {
        if let Some((api, permission)) = self.hook() {
            api.revoke_session_permission("fixture", permission).await;
        }
        guard()?;
        let mut covers = self.covers.lock().unwrap();
        let (current, revision) = covers.get(&entry_key).cloned().unwrap_or((None, 0));
        if revision != expected_revision {
            return Ok(ArtworkWrite::Conflict { current: revision });
        }
        if current == image {
            return Ok(ArtworkWrite::Unchanged { revision });
        }
        covers.insert(entry_key, (image, revision + 1));
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(ArtworkWrite::Applied {
            revision: revision + 1,
            replaced: None,
        })
    }

    async fn set_display_name(
        &self,
        entry_key: String,
        expected_revision: u64,
        display_name: Option<String>,
        guard: CommitGuard,
    ) -> Result<DisplayWrite, Status> {
        if let Some((api, permission)) = self.hook() {
            api.revoke_session_permission("fixture", permission).await;
        }
        guard()?;
        let mut names = self.names.lock().unwrap();
        let (current, revision) = names.get(&entry_key).cloned().unwrap_or((None, 0));
        if revision != expected_revision {
            return Ok(DisplayWrite::Conflict { current: revision });
        }
        if current == display_name {
            return Ok(DisplayWrite::Unchanged { revision });
        }
        names.insert(entry_key, (display_name, revision + 1));
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(DisplayWrite::Applied {
            revision: revision + 1,
        })
    }
}

impl Fixture {
    /// Connects a fresh `FakeHost` as the engine side.
    fn with_host(&self) -> Arc<FakeHost> {
        let host = Arc::new(FakeHost::default());
        self.feed.set_host(host.clone());
        host
    }

    fn mutation(&self, key: &str, expected_revision: u64) -> MutationContext {
        MutationContext {
            context: Some(self.context()),
            operation_id: String::new(),
            idempotency_key: key.into(),
            expected_revision,
        }
    }

    async fn set_name(
        &self,
        key: &str,
        expected_revision: u64,
        installation_id: &str,
        game_id: &str,
        fields: &[(&str, &str)],
    ) -> Result<MutationReply, Status> {
        self.client()
            .metadata_update(MetadataUpdateRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                game_id: game_id.into(),
                installation_id: installation_id.into(),
                display_fields: fields
                    .iter()
                    .map(|(name, value)| ((*name).into(), (*value).into()))
                    .collect(),
            })
            .await
            .map(Response::into_inner)
    }

    /// Sets the display name of `id-fixture:1` with the given key and expected revision.
    async fn rename(&self, key: &str, expected: u64, name: &str) -> Result<MutationReply, Status> {
        self.set_name(key, expected, "id-fixture:1", "", &[("display_name", name)])
            .await
    }

    async fn favorite(
        &self,
        key: &str,
        expected_revision: u64,
        profile_id: &str,
        game_id: &str,
        favorite: bool,
    ) -> Result<MutationReply, Status> {
        self.client()
            .favorites_set(FavoritesSetRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                profile_id: profile_id.into(),
                game_id: game_id.into(),
                favorite,
            })
            .await
            .map(Response::into_inner)
    }

    fn favorites_request(
        &self,
        page_size: u32,
        cursor: &str,
        profile_id: &str,
    ) -> FavoritesListRequest {
        FavoritesListRequest {
            page: Some(PageRequest {
                context: Some(self.context()),
                cursor: cursor.into(),
                page_size,
                common_data_categories: None,
            }),
            profile_id: profile_id.into(),
        }
    }

    async fn favorites(
        &self,
        page_size: u32,
        cursor: &str,
        profile_id: &str,
    ) -> Result<FavoritesListReply, Status> {
        self.client()
            .favorites_list(self.favorites_request(page_size, cursor, profile_id))
            .await
            .map(Response::into_inner)
    }
}

fn outcome(reply: &MutationReply) -> MutationOutcome {
    MutationOutcome::try_from(reply.outcome).unwrap()
}

fn games(reply: &FavoritesListReply) -> Vec<(&str, bool)> {
    reply
        .entries
        .iter()
        .map(|entry| (entry.game_id.as_str(), entry.favorite))
        .collect()
}

#[tokio::test]
async fn display_names_are_applied_once_idempotent_and_guarded_by_their_revision() {
    let fixture = Fixture::new(&[cap::LIBRARY_METADATA_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();

    let first = fixture.rename("k1", 0, "Portal Two").await.unwrap();
    assert_eq!(outcome(&first), MutationOutcome::Applied);
    assert_eq!(
        (first.resource_id.as_str(), first.revision),
        ("id-fixture:1", 1)
    );
    assert_eq!(
        host.name_of("fixture:1"),
        Some((Some("Portal Two".into()), 1))
    );

    // The same key with the same request returns the original outcome without writing again.
    assert_eq!(fixture.rename("k1", 0, "Portal Two").await.unwrap(), first);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // The same key for another request is refused, whatever its content.
    for other in [
        rename_request(&fixture, "k1", 1, "Other"),
        rename_request(&fixture, "k1", 0, "Portal 2"),
    ] {
        let status = other.await.unwrap_err();
        assert_eq!(status.code(), Code::FailedPrecondition);
    }

    // A new key writing the value that is already there changes nothing.
    let unchanged = fixture.rename("k2", 1, "Portal Two").await.unwrap();
    assert_eq!(outcome(&unchanged), MutationOutcome::AlreadyApplied);
    assert_eq!(unchanged.revision, 1);

    // An obsolete revision reports the current one and never overwrites the newer value.
    let conflict = fixture.rename("k3", 0, "Stale write").await.unwrap();
    assert_eq!(outcome(&conflict), MutationOutcome::RevisionConflict);
    assert_eq!(conflict.revision, 1);
    assert_eq!(
        host.name_of("fixture:1"),
        Some((Some("Portal Two".into()), 1))
    );
    // A conflict is not a committed outcome: its key can be used again after re-reading.
    let retried = fixture.rename("k3", 1, "Stale write").await.unwrap();
    assert_eq!(
        (outcome(&retried), retried.revision),
        (MutationOutcome::Applied, 2)
    );

    // An empty (or blank) value restores the detected name; the revision keeps counting.
    let reset = fixture.rename("k4", 2, "  ").await.unwrap();
    assert_eq!(
        (outcome(&reset), reset.revision),
        (MutationOutcome::Applied, 3)
    );
    assert_eq!(host.name_of("fixture:1"), Some((None, 3)));
    assert_eq!(host.writes.load(Ordering::SeqCst), 3);
}

/// A rename that has not been awaited yet, so a loop can drive several.
async fn rename_request(
    fixture: &Fixture,
    key: &str,
    expected: u64,
    name: &str,
) -> Result<MutationReply, Status> {
    fixture.rename(key, expected, name).await
}

#[tokio::test]
async fn display_metadata_requests_are_validated_before_anything_is_written() {
    let fixture = Fixture::new(&[cap::LIBRARY_METADATA_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let invalid = |status: Status, what: &str| {
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status:?}");
    };

    let status = fixture
        .client()
        .metadata_update(MetadataUpdateRequest {
            mutation: None,
            installation_id: "id-fixture:1".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    invalid(status, "missing mutation");
    for key in ["", "has\ncontrol", &"k".repeat(129)] {
        let status = fixture.rename(key, 0, "Name").await.unwrap_err();
        invalid(status, "idempotency key");
    }
    let mut long_operation = fixture.mutation("k", 0);
    long_operation.operation_id = "o".repeat(129);
    let status = fixture
        .client()
        .metadata_update(MetadataUpdateRequest {
            mutation: Some(long_operation),
            installation_id: "id-fixture:1".into(),
            display_fields: HashMap::from([("display_name".to_owned(), "N".to_owned())]),
            ..Default::default()
        })
        .await
        .unwrap_err();
    invalid(status, "operation id");

    let status = fixture
        .set_name("a", 0, "id-fixture:1", "", &[])
        .await
        .unwrap_err();
    invalid(status, "no fields");
    let status = fixture
        .set_name("b", 0, "id-fixture:1", "", &[("sort_title", "x")])
        .await
        .unwrap_err();
    invalid(status, "unknown field");
    assert!(status_message_names(&fixture, "c", "sort_title").await);
    let status = fixture
        .set_name(
            "d",
            0,
            "id-fixture:1",
            "",
            &[("display_name", "ok"), ("notes", "x")],
        )
        .await
        .unwrap_err();
    invalid(status, "a writable field next to an unknown one");
    invalid(
        fixture.rename("e", 0, &"x".repeat(257)).await.unwrap_err(),
        "257 bytes",
    );
    invalid(
        fixture.rename("f", 0, "tab\there").await.unwrap_err(),
        "control characters",
    );
    invalid(
        fixture
            .set_name("g", 0, "", "", &[("display_name", "N")])
            .await
            .unwrap_err(),
        "no installation",
    );
    invalid(
        fixture
            .set_name("h", 0, "id-fixture:1", "seven", &[("display_name", "N")])
            .await
            .unwrap_err(),
        "game id",
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.name_of("fixture:1").is_none());

    // The longest accepted name is 256 bytes, and surrounding blanks are trimmed.
    let longest = "x".repeat(256);
    let reply = fixture
        .rename("i", 0, &format!("  {longest}  "))
        .await
        .unwrap();
    assert_eq!(outcome(&reply), MutationOutcome::Applied);
    assert_eq!(host.name_of("fixture:1").unwrap().0, Some(longest));
}

/// The error for a non-writable field names it and the one writable field.
async fn status_message_names(fixture: &Fixture, key: &str, field: &str) -> bool {
    let status = fixture
        .set_name(key, 0, "id-fixture:1", "", &[(field, "x")])
        .await
        .unwrap_err();
    status.message().contains(field) && status.message().contains("display_name")
}

#[tokio::test]
async fn display_metadata_writes_never_reach_another_providers_entries() {
    let fixture = Fixture::new(&[
        cap::LIBRARY_METADATA_WRITE,
        cap::LIBRARY_READ,
        cap::LIBRARY_READ_ALL,
    ])
    .await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    // Reading the whole library does not make another provider's entry writable, and the
    // refusal is the same as for an installation that does not exist.
    assert_eq!(
        fixture.get("id-steam:620", "").await.unwrap().name,
        "Portal 2"
    );
    for id in ["id-steam:620", "id-steam:570", "id-nope"] {
        let status = fixture
            .set_name("k", 0, id, "", &[("display_name", "Mine")])
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{id}");
        assert_eq!(status.message(), "library entry not found");
    }
    // A game id that disagrees with the installation names no entry; one that agrees works.
    let status = fixture
        .set_name("k", 0, "id-fixture:1", "8", &[("display_name", "Mine")])
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    let reply = fixture
        .set_name("k", 0, "id-fixture:1", "7", &[("display_name", "Mine")])
        .await
        .unwrap();
    assert_eq!(outcome(&reply), MutationOutcome::Applied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    assert!(host.name_of("steam:620").is_none());
}

#[tokio::test]
async fn display_metadata_needs_its_grant_and_the_engine_side() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let status = fixture.rename("k", 0, "Mine").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::LIBRARY_METADATA_WRITE)
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    // With the grant but no engine side connected, the service is unavailable.
    let fixture = Fixture::new(&[cap::LIBRARY_METADATA_WRITE]).await;
    fixture.library(SHARED);
    let status = fixture.rename("k", 0, "Mine").await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_display_name_is_not_committed_when_authorization_is_lost_before_commit() {
    // The permission itself is revoked between the check and the commit.
    let fixture = Fixture::new(&[cap::LIBRARY_METADATA_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    *host.revoke_before_commit.lock().unwrap() =
        Some((fixture.api.clone(), cap::LIBRARY_METADATA_WRITE));
    let status = fixture.rename("k", 0, "Mine").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.name_of("fixture:1").is_none());

    // Any other grant change moves the grant revision, which also stops the commit.
    let fixture = Fixture::new(&[cap::LIBRARY_METADATA_WRITE, cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    *host.revoke_before_commit.lock().unwrap() = Some((fixture.api.clone(), cap::LIBRARY_READ));
    let status = fixture.rename("k", 0, "Mine").await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(status.message().contains("permissions changed"));
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn favorites_list_shows_only_games_matched_by_the_callers_own_entries() {
    let fixture = Fixture::new(&[cap::FAVORITES_READ, cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    {
        let mut games = host.games.lock().unwrap();
        for (provider, game, favorite) in [
            ("fixture", 7, true),
            ("fixture", 12, true),
            ("fixture", 9, false),
            ("steam", 5, true),
            ("steam", 7, true),
        ] {
            games.insert((provider.to_owned(), game), favorite);
        }
    }

    // Numeric order, no other provider's games, and no revision to guard favorites with.
    let reply = fixture.favorites(0, "", "").await.unwrap();
    assert_eq!(games(&reply), [("7", true), ("9", false), ("12", true)]);
    assert!(reply.entries.iter().all(|entry| entry.revision == 0));
    assert_eq!(reply.revision, fixture.feed.revision());
    assert!(reply.next_cursor.is_empty());
    // An empty selector is the default query; a named profile must be the active one.
    let mut request = fixture.favorites_request(0, "", "");
    request.page.as_mut().unwrap().common_data_categories = Some(CommonDataSelector::default());
    assert!(fixture.client().favorites_list(request).await.is_ok());
    let mut request = fixture.favorites_request(0, "", "");
    request.page.as_mut().unwrap().common_data_categories = Some(CommonDataSelector {
        categories: vec![CommonDataCategory::GamePaths as i32],
        selected_record_ids: vec![],
    });
    let status = fixture.client().favorites_list(request).await.unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    let active = crate::plugin_host::profile::profile_id_for_account("a");
    assert!(fixture.favorites(0, "", &active).await.is_ok());
    for wrong in [
        "nope".to_owned(),
        crate::plugin_host::profile::profile_id_for_account("b"),
    ] {
        let status = fixture.favorites(0, "", &wrong).await.unwrap_err();
        assert_eq!(status.code(), Code::FailedPrecondition, "{wrong}");
    }

    // Pages continue after the last game id.
    let first = fixture.favorites(2, "", "").await.unwrap();
    assert_eq!(games(&first), [("7", true), ("9", false)]);
    assert!(!first.next_cursor.is_empty());
    let second = fixture.favorites(2, &first.next_cursor, "").await.unwrap();
    assert_eq!(games(&second), [("12", true)]);
    assert!(second.next_cursor.is_empty());
    let exact = fixture.favorites(3, "", "").await.unwrap();
    assert!(
        exact.next_cursor.is_empty(),
        "no cursor after the last entry"
    );
}

#[tokio::test]
async fn favorites_cursors_are_bound_to_their_list_and_the_library_revision() {
    let fixture = Fixture::new(&[cap::FAVORITES_READ, cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    for game in [1, 2, 3] {
        host.games
            .lock()
            .unwrap()
            .insert(("fixture".to_owned(), game), false);
    }
    let favorites = fixture.favorites(1, "", "").await.unwrap();
    let library = fixture
        .client()
        .library_list(fixture.list_request(1, "", ""))
        .await
        .unwrap()
        .into_inner();
    assert!(!favorites.next_cursor.is_empty() && !library.next_cursor.is_empty());

    // Neither list accepts the other's cursor.
    let status = fixture
        .favorites(1, &library.next_cursor, "")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let status = fixture
        .client()
        .library_list(fixture.list_request(1, &favorites.next_cursor, ""))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    // A forged cursor is not a cursor.
    let status = fixture.favorites(1, "00.00", "").await.unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);

    // The library changing makes the cursor stale and returns no entries.
    assert!(
        fixture
            .favorites(1, &favorites.next_cursor, "")
            .await
            .is_ok()
    );
    fixture.library(&SHARED[..3]);
    let status = fixture
        .favorites(1, &favorites.next_cursor, "")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(status.message().contains("restart the list"));
}

#[tokio::test]
async fn favorites_list_needs_its_grant_and_the_engine_side() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    fixture.with_host();
    let status = fixture.favorites(0, "", "").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::FAVORITES_READ)
    );

    let fixture = Fixture::new(&[cap::FAVORITES_READ]).await;
    let status = fixture.favorites(0, "", "").await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    let status = fixture
        .client()
        .favorites_list(FavoritesListRequest::default())
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument, "page is required");
}

#[tokio::test]
async fn favorites_are_set_on_the_game_idempotently_for_own_matched_entries_only() {
    let fixture = Fixture::new(&[cap::FAVORITES_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    {
        let mut games = host.games.lock().unwrap();
        games.insert(("fixture".to_owned(), 7), false);
        // Only another provider's entry is matched to game 9.
        games.insert(("steam".to_owned(), 9), false);
    }

    let applied = fixture.favorite("k1", 0, "", "7", true).await.unwrap();
    assert_eq!(outcome(&applied), MutationOutcome::Applied);
    assert_eq!((applied.resource_id.as_str(), applied.revision), ("7", 0));
    assert_eq!(host.favorites("fixture"), [(7, true)]);
    // The same key and request returns the original outcome and writes nothing more.
    assert_eq!(
        fixture.favorite("k1", 0, "", "7", true).await.unwrap(),
        applied
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // Another request with that key is refused.
    let status = fixture.favorite("k1", 0, "", "7", false).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(host.favorites("fixture"), [(7, true)]);
    // A new key for the state that already holds changes nothing.
    let same = fixture.favorite("k2", 0, "", "7", true).await.unwrap();
    assert_eq!(outcome(&same), MutationOutcome::AlreadyApplied);
    let off = fixture.favorite("k3", 0, "", "7", false).await.unwrap();
    assert_eq!(outcome(&off), MutationOutcome::Applied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 2);

    // No own entry matched: unknown game and another provider's game look the same.
    for game in ["8", "9"] {
        let status = fixture.favorite("k4", 0, "", game, true).await.unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{game}");
    }
    assert_eq!(host.favorites("steam"), [(9, false)]);

    // Shape checks.
    let invalid = |status: Status, what: &str| {
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status:?}");
    };
    invalid(
        fixture.favorite("k5", 1, "", "7", true).await.unwrap_err(),
        "favorites carry no revision",
    );
    invalid(
        fixture.favorite("", 0, "", "7", true).await.unwrap_err(),
        "key",
    );
    invalid(
        fixture.favorite("k6", 0, "", "", true).await.unwrap_err(),
        "game id",
    );
    invalid(
        fixture
            .favorite("k7", 0, "", "seven", true)
            .await
            .unwrap_err(),
        "game id",
    );
    let status = fixture
        .client()
        .favorites_set(FavoritesSetRequest::default())
        .await
        .unwrap_err();
    invalid(status, "missing mutation");
    let status = fixture
        .favorite("k8", 0, "nope", "7", true)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let active = crate::plugin_host::profile::profile_id_for_account("a");
    assert!(fixture.favorite("k9", 0, &active, "7", true).await.is_ok());
    assert_eq!(host.writes.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn favorites_set_needs_its_grant_and_the_engine_side() {
    let fixture = Fixture::new(&[cap::FAVORITES_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.games
        .lock()
        .unwrap()
        .insert(("fixture".to_owned(), 7), false);
    let status = fixture.favorite("k", 0, "", "7", true).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::FAVORITES_WRITE)
    );
    assert_eq!(host.favorites("fixture"), [(7, false)]);

    let fixture = Fixture::new(&[cap::FAVORITES_WRITE]).await;
    let status = fixture.favorite("k", 0, "", "7", true).await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_favorite_is_not_committed_when_authorization_is_lost_before_commit() {
    let fixture = Fixture::new(&[cap::FAVORITES_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.games
        .lock()
        .unwrap()
        .insert(("fixture".to_owned(), 7), false);
    *host.revoke_before_commit.lock().unwrap() = Some((fixture.api.clone(), cap::FAVORITES_WRITE));
    let status = fixture.favorite("k", 0, "", "7", true).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(host.favorites("fixture"), [(7, false)]);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

// ---- Matching (task 5.3) ----

fn catalog_game(id: u64, name: &str, year: Option<i32>) -> CatalogGame {
    CatalogGame {
        id,
        name: name.into(),
        release_year: year,
    }
}

impl Fixture {
    async fn search(&self, query: &str, page_size: u32) -> Result<MatchingSearchReply, Status> {
        self.client()
            .matching_search(MatchingSearchRequest {
                context: Some(self.context()),
                query: query.into(),
                platform: String::new(),
                page_size,
                cursor: String::new(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn assign_request(
        &self,
        key: &str,
        expected_revision: u64,
        installation_id: &str,
        game_id: &str,
    ) -> Result<MutationReply, Status> {
        self.assign_with(MatchingAssignRequest {
            mutation: Some(self.mutation(key, expected_revision)),
            installation_id: installation_id.into(),
            gamenode_game_id: game_id.into(),
            ..MatchingAssignRequest::default()
        })
        .await
    }

    async fn assign_with(&self, request: MatchingAssignRequest) -> Result<MutationReply, Status> {
        self.client()
            .matching_assign(request)
            .await
            .map(Response::into_inner)
    }

    /// Assigns game `game` to `id-fixture:2` with a key and expected revision.
    async fn assign(&self, key: &str, expected: u64, game: &str) -> Result<MutationReply, Status> {
        self.assign_request(key, expected, "id-fixture:2", game)
            .await
    }
}

#[tokio::test]
async fn a_catalog_search_returns_a_bounded_page_of_typed_candidates() {
    let fixture = Fixture::new(&[cap::MATCHING_READ]).await;
    let host = fixture.with_host();
    *host.catalog.lock().unwrap() = (1..=60)
        .map(|id| catalog_game(id, &format!("Portal {id}"), (id % 2 == 0).then_some(2007)))
        .chain([catalog_game(900, "Dota 2", Some(2013))])
        .collect();

    let found = fixture.search("  dota ", 0).await.unwrap();
    assert_eq!(
        found.candidates,
        [MatchCandidate {
            game_id: "900".into(),
            title: "Dota 2".into(),
            platform: String::new(),
            confidence: String::new(),
            release_year: 2013,
        }]
    );
    assert_eq!(found.next_cursor, "");

    // The default page is 20, the largest 50, and a smaller request is honored.
    assert_eq!(
        fixture.search("portal", 0).await.unwrap().candidates.len(),
        20
    );
    assert_eq!(
        fixture.search("portal", 7).await.unwrap().candidates.len(),
        7
    );
    assert_eq!(
        fixture
            .search("portal", 5_000)
            .await
            .unwrap()
            .candidates
            .len(),
        50
    );
    // An unknown release year is 0, never made up.
    let unknown = fixture.search("Portal 1", 1).await.unwrap();
    assert_eq!(unknown.candidates[0].release_year, 0);
    let none = fixture.search("zzz", 0).await.unwrap();
    assert!(none.candidates.is_empty());
}

#[tokio::test]
async fn catalog_search_requests_are_validated_before_the_catalog_is_asked() {
    let fixture = Fixture::new(&[cap::MATCHING_READ]).await;
    let host = fixture.with_host();
    let invalid = |status: Status, what: &str| {
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status:?}");
    };
    invalid(fixture.search("", 0).await.unwrap_err(), "empty query");
    invalid(fixture.search("   ", 0).await.unwrap_err(), "blank query");
    invalid(
        fixture.search(&"a".repeat(129), 0).await.unwrap_err(),
        "long query",
    );
    invalid(
        fixture.search("po\u{7}rtal", 0).await.unwrap_err(),
        "control character",
    );
    assert!(fixture.search(&"a".repeat(128), 0).await.is_ok());
    let before = host.searches.load(Ordering::SeqCst);
    for (platform, cursor) in [("pc", ""), ("", "next")] {
        let status = fixture
            .client()
            .matching_search(MatchingSearchRequest {
                context: Some(fixture.context()),
                query: "portal".into(),
                platform: platform.into(),
                page_size: 0,
                cursor: cursor.into(),
            })
            .await
            .unwrap_err();
        invalid(status, "reserved field");
    }
    assert_eq!(host.searches.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn an_unavailable_catalog_is_unavailable_not_empty() {
    let fixture = Fixture::new(&[cap::MATCHING_READ]).await;
    // No engine side at all.
    let status = fixture.search("portal", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    // The engine could not reach the catalog.
    let host = fixture.with_host();
    host.catalog_down.store(true, Ordering::SeqCst);
    let status = fixture.search("portal", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    host.catalog_down.store(false, Ordering::SeqCst);
    assert!(
        fixture
            .search("portal", 0)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn catalog_searches_need_the_grant_and_are_rate_limited() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.with_host();
    let status = fixture.search("portal", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::MATCHING_READ)
    );

    let fixture = Fixture::new(&[cap::MATCHING_READ]).await;
    let host = fixture.with_host();
    for _ in 0..SEARCHES_PER_WINDOW_FOR_TEST {
        fixture.search("portal", 0).await.unwrap();
    }
    let status = fixture.search("portal", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(
        host.searches.load(Ordering::SeqCst),
        SEARCHES_PER_WINDOW_FOR_TEST,
        "a limited search never reaches the catalog"
    );
}

const SEARCHES_PER_WINDOW_FOR_TEST: usize = super::matching::SEARCHES_PER_WINDOW;

#[tokio::test]
async fn an_own_entry_is_assigned_once_idempotently_and_guarded_by_its_revision() {
    let fixture = Fixture::new(&[cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().extend([7, 8]);

    let first = fixture.assign("k1", 1, "7").await.unwrap();
    assert_eq!(outcome(&first), MutationOutcome::Applied);
    assert_eq!(
        (first.resource_id.as_str(), first.revision),
        ("id-fixture:2", 2)
    );
    // The same key and request returns the original outcome without writing again.
    assert_eq!(fixture.assign("k1", 1, "7").await.unwrap(), first);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // The same key for another request is refused.
    for other in [fixture.assign("k1", 1, "8"), fixture.assign("k1", 2, "7")] {
        assert_eq!(other.await.unwrap_err().code(), Code::FailedPrecondition);
    }
    // A new key for the state that already holds changes nothing, whatever revision it names.
    let same = fixture.assign("k2", 2, "7").await.unwrap();
    assert_eq!(outcome(&same), MutationOutcome::AlreadyApplied);
    assert_eq!(same.revision, 2);

    // A stale revision reports the current one and writes nothing; it is not remembered, so the
    // same key can retry with the right revision.
    let stale = fixture.assign("k3", 1, "8").await.unwrap();
    assert_eq!(outcome(&stale), MutationOutcome::RevisionConflict);
    assert_eq!(stale.revision, 2);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    let retried = fixture.assign("k3", 2, "8").await.unwrap();
    assert_eq!(outcome(&retried), MutationOutcome::Applied);
    assert_eq!(retried.revision, 3);
}

#[tokio::test]
async fn a_game_must_be_known_or_found_by_a_search_before_it_is_assigned() {
    let fixture = Fixture::new(&[cap::MATCHING_READ, cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    *host.catalog.lock().unwrap() = vec![catalog_game(40, "Defense of the Ancients", None)];

    let status = fixture.assign("k1", 1, "40").await.unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    fixture.search("defense", 0).await.unwrap();
    let applied = fixture.assign("k1", 1, "40").await.unwrap();
    assert_eq!(outcome(&applied), MutationOutcome::Applied);
}

#[tokio::test]
async fn a_match_the_user_chose_is_not_replaced_and_the_refusal_is_not_remembered() {
    let fixture = Fixture::new(&[cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().extend([7, 8]);
    host.matches
        .lock()
        .unwrap()
        .insert("fixture:2".to_owned(), (Some(7), true, 1));

    let status = fixture.assign("k1", 1, "8").await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    // Choosing the game the user already chose changes nothing.
    let same = fixture.assign("k2", 1, "7").await.unwrap();
    assert_eq!(outcome(&same), MutationOutcome::AlreadyApplied);
    // When the user lets go (the choice is removed) the same key is not poisoned.
    host.matches.lock().unwrap().remove("fixture:2");
    let applied = fixture.assign("k1", 1, "8").await.unwrap();
    assert_eq!(outcome(&applied), MutationOutcome::Applied);
}

#[tokio::test]
async fn only_own_entries_are_assigned_even_with_whole_library_reads() {
    let fixture = Fixture::new(&[cap::MATCHING_WRITE, cap::LIBRARY_READ_ALL]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().insert(7);
    for installation in ["id-steam:570", "id-steam:620", "id-fixture:nope", "nope"] {
        let status = fixture
            .assign_request("k", 1, installation, "7")
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{installation}");
    }
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.matches.lock().unwrap().is_empty());
}

#[tokio::test]
async fn assign_requests_are_validated_before_anything_is_written() {
    let fixture = Fixture::new(&[cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().insert(7);
    let invalid = |status: Status, what: &str| {
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status:?}");
    };
    invalid(fixture.assign("", 1, "7").await.unwrap_err(), "key");
    invalid(fixture.assign("k", 1, "").await.unwrap_err(), "game id");
    invalid(
        fixture.assign("k", 1, "seven").await.unwrap_err(),
        "game id",
    );
    invalid(
        fixture.assign_request("k", 1, "", "7").await.unwrap_err(),
        "installation id",
    );
    invalid(
        fixture
            .assign_request("k", 1, &"i".repeat(129), "7")
            .await
            .unwrap_err(),
        "installation id length",
    );
    for (provider, game) in [("fixture", ""), ("", "2")] {
        let status = fixture
            .assign_with(MatchingAssignRequest {
                mutation: Some(fixture.mutation("k", 1)),
                provider_id: provider.into(),
                provider_game_id: game.into(),
                gamenode_game_id: "7".into(),
                installation_id: "id-fixture:2".into(),
            })
            .await
            .unwrap_err();
        invalid(status, "reserved fields");
    }
    invalid(
        fixture
            .client()
            .matching_assign(MatchingAssignRequest::default())
            .await
            .unwrap_err(),
        "missing mutation",
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn assign_needs_its_grant_and_the_engine_side() {
    let fixture = Fixture::new(&[cap::MATCHING_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().insert(7);
    let status = fixture.assign("k", 1, "7").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::MATCHING_WRITE)
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    let fixture = Fixture::new(&[cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let status = fixture.assign("k", 1, "7").await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_match_is_not_committed_when_authorization_is_lost_before_commit() {
    let fixture = Fixture::new(&[cap::MATCHING_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.known.lock().unwrap().insert(7);
    *host.revoke_before_commit.lock().unwrap() = Some((fixture.api.clone(), cap::MATCHING_WRITE));
    let status = fixture.assign("k", 1, "7").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.matches.lock().unwrap().is_empty());
}

// ---- Artwork (task 5.4) ----

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([20, 120, 220]),
    ))
    .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
    .unwrap();
    out
}

impl Fixture {
    async fn set_cover(
        &self,
        key: &str,
        expected_revision: u64,
        installation_id: &str,
        image: Vec<u8>,
        media_type: &str,
    ) -> Result<MutationReply, Status> {
        self.set_cover_with(ArtworkSetRequest {
            mutation: Some(self.mutation(key, expected_revision)),
            installation_id: installation_id.into(),
            encoded_image: image,
            media_type: media_type.into(),
            ..ArtworkSetRequest::default()
        })
        .await
    }

    async fn set_cover_with(&self, request: ArtworkSetRequest) -> Result<MutationReply, Status> {
        self.client()
            .artwork_set(request)
            .await
            .map(Response::into_inner)
    }

    async fn read_cover(
        &self,
        installation_id: &str,
        max_bytes: u64,
    ) -> Result<ArtworkReadReply, Status> {
        self.client()
            .artwork_read(ArtworkReadRequest {
                context: Some(self.context()),
                installation_id: installation_id.into(),
                max_bytes,
                ..ArtworkReadRequest::default()
            })
            .await
            .map(Response::into_inner)
    }
}

#[tokio::test]
async fn an_own_entrys_cover_is_set_once_idempotently_and_guarded_by_its_revision() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();

    let applied = fixture
        .set_cover("k1", 0, "id-fixture:1", png(900, 1200), "image/png")
        .await
        .unwrap();
    assert_eq!(
        (
            applied.resource_id.as_str(),
            applied.revision,
            applied.outcome
        ),
        ("id-fixture:1", 1, MutationOutcome::Applied as i32)
    );
    // The host stored its own smaller, re-encoded picture, not the plugin's bytes.
    let stored = host.covers.lock().unwrap()["fixture:1"].0.clone().unwrap();
    assert_eq!((stored.width, stored.height), (600, 800));
    assert_eq!(stored.media_type, "image/jpeg");

    // The same key and request replays the first answer without another write.
    let again = fixture
        .set_cover("k1", 0, "id-fixture:1", png(900, 1200), "image/png")
        .await
        .unwrap();
    assert_eq!(again, applied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // The same key for another picture is refused.
    let status = fixture
        .set_cover("k1", 0, "id-fixture:1", png(10, 10), "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);

    // A stale revision conflicts and writes nothing; the refusal is not remembered.
    let conflict = fixture
        .set_cover("k2", 0, "id-fixture:1", png(40, 60), "image/png")
        .await
        .unwrap();
    assert_eq!(
        (conflict.revision, conflict.outcome),
        (1, MutationOutcome::RevisionConflict as i32)
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    let retried = fixture
        .set_cover("k2", 1, "id-fixture:1", png(40, 60), "image/png")
        .await
        .unwrap();
    assert_eq!(
        (retried.revision, retried.outcome),
        (2, MutationOutcome::Applied as i32)
    );
    // Setting what the entry already has is already applied.
    let same = fixture
        .set_cover("k3", 2, "id-fixture:1", png(40, 60), "image/png")
        .await
        .unwrap();
    assert_eq!(
        (same.revision, same.outcome),
        (2, MutationOutcome::AlreadyApplied as i32)
    );
}

#[tokio::test]
async fn an_empty_image_restores_the_games_own_cover() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE, cap::ARTWORK_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.game_covers
        .lock()
        .unwrap()
        .insert("fixture:1".into(), b"game cover".to_vec());
    fixture
        .set_cover("k1", 0, "id-fixture:1", png(40, 60), "image/png")
        .await
        .unwrap();
    let cleared = fixture
        .set_cover("k2", 1, "id-fixture:1", Vec::new(), "")
        .await
        .unwrap();
    assert_eq!(
        (cleared.revision, cleared.outcome),
        (2, MutationOutcome::Applied as i32)
    );
    let read = fixture.read_cover("id-fixture:1", 0).await.unwrap();
    assert_eq!(read.encoded_image, b"game cover");
    // Clearing again changes nothing.
    let again = fixture
        .set_cover("k3", 2, "id-fixture:1", Vec::new(), "")
        .await
        .unwrap();
    assert_eq!(again.outcome, MutationOutcome::AlreadyApplied as i32);
    // A media type with nothing to describe is a mistake, not a clear.
    let status = fixture
        .set_cover("k4", 2, "id-fixture:1", Vec::new(), "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn bad_images_never_reach_the_engine() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let heavy = {
        let mut out = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::new(4097, 4097))
            .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
            .unwrap();
        out
    };
    let mut half = png(64, 64);
    half.truncate(half.len() / 2);
    let mut oversized = png(8, 8);
    oversized.resize(gamenode_platform::plugin_artwork::MAX_INPUT_BYTES + 1, 0);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "not an image",
            b"#!/bin/sh\nrm -rf /\n".to_vec(),
            "image/png",
        ),
        (
            "svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
            "image/svg+xml",
        ),
        ("lying about the type", png(8, 8), "image/jpeg"),
        ("no type", png(8, 8), ""),
        ("truncated", half, "image/png"),
        ("too many pixels", heavy, "image/png"),
        ("too many bytes", oversized, "image/png"),
    ];
    for (index, (what, bytes, media_type)) in cases.into_iter().enumerate() {
        let status = fixture
            .set_cover(
                &format!("bad-{index}"),
                0,
                "id-fixture:1",
                bytes,
                media_type,
            )
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status}");
    }
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.covers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn covers_are_set_only_on_own_entries_that_exist() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE, cap::LIBRARY_READ_ALL]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    // Another provider's entry and an unknown one are the same refusal, even with whole-library
    // reads; the image is not even decoded.
    for (key, id) in [("a", "id-steam:620"), ("b", "id-nope")] {
        let status = fixture
            .set_cover(key, 0, id, png(40, 60), "image/png")
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{id}");
    }
    // A game id the entry is not matched to does not name it either.
    let status = fixture
        .set_cover_with(ArtworkSetRequest {
            mutation: Some(fixture.mutation("c", 0)),
            installation_id: "id-fixture:1".into(),
            game_id: "999".into(),
            encoded_image: png(40, 60),
            media_type: "image/png".into(),
            ..ArtworkSetRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    // Naming the game it is matched to is fine.
    fixture
        .set_cover_with(ArtworkSetRequest {
            mutation: Some(fixture.mutation("d", 0)),
            installation_id: "id-fixture:1".into(),
            game_id: "7".into(),
            encoded_image: png(40, 60),
            media_type: "image/png".into(),
            ..ArtworkSetRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    assert_eq!(host.covers.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cover_set_requests_are_validated_before_anything_is_written() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let base = |mutation: Option<MutationContext>| ArtworkSetRequest {
        mutation,
        installation_id: "id-fixture:1".into(),
        encoded_image: png(8, 8),
        media_type: "image/png".into(),
        ..ArtworkSetRequest::default()
    };
    let cases = vec![
        ("no mutation", base(None)),
        ("no key", base(Some(fixture.mutation("", 0)))),
        (
            "no installation",
            ArtworkSetRequest {
                installation_id: String::new(),
                ..base(Some(fixture.mutation("k", 0)))
            },
        ),
        (
            "long installation",
            ArtworkSetRequest {
                installation_id: "x".repeat(129),
                ..base(Some(fixture.mutation("k", 0)))
            },
        ),
        (
            "another kind",
            ArtworkSetRequest {
                artwork_kind: "banner".into(),
                ..base(Some(fixture.mutation("k", 0)))
            },
        ),
        (
            "bad game id",
            ArtworkSetRequest {
                game_id: "seven".into(),
                ..base(Some(fixture.mutation("k", 0)))
            },
        ),
    ];
    for (what, request) in cases {
        let status = fixture.set_cover_with(request).await.unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status}");
    }
    // "cover" is the one kind there is.
    fixture
        .set_cover_with(ArtworkSetRequest {
            artwork_kind: "cover".into(),
            ..base(Some(fixture.mutation("ok", 0)))
        })
        .await
        .unwrap();
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cover_calls_need_their_grants_and_the_engine_side() {
    let fixture = Fixture::new(&[cap::ARTWORK_READ]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let status = fixture
        .set_cover("k", 0, "id-fixture:1", png(8, 8), "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .api
            .authorization_audit()
            .iter()
            .any(|event| event.permission_id == cap::ARTWORK_WRITE)
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    let fixture = Fixture::new(&[cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    let status = fixture.read_cover("id-fixture:1", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let status = fixture
        .set_cover("k", 0, "id-fixture:1", png(8, 8), "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cover_is_not_committed_when_authorization_is_lost_before_commit() {
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    *host.revoke_before_commit.lock().unwrap() = Some((fixture.api.clone(), cap::ARTWORK_WRITE));
    let status = fixture
        .set_cover("k", 0, "id-fixture:1", png(40, 60), "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.covers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_own_entrys_cover_is_read_within_the_callers_budget() {
    let fixture = Fixture::new(&[cap::ARTWORK_READ, cap::LIBRARY_READ_ALL]).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    host.game_covers
        .lock()
        .unwrap()
        .insert("fixture:1".into(), b"0123456789".to_vec());
    host.game_covers
        .lock()
        .unwrap()
        .insert("steam:620".into(), b"foreign".to_vec());

    let read = fixture.read_cover("id-fixture:1", 0).await.unwrap();
    assert_eq!(
        (
            read.encoded_image.as_slice(),
            read.media_type.as_str(),
            read.byte_length,
            read.revision
        ),
        (b"0123456789".as_slice(), "image/jpeg", 10, 0)
    );
    assert_eq!(
        fixture
            .read_cover("id-fixture:1", 10)
            .await
            .unwrap()
            .byte_length,
        10
    );
    let status = fixture.read_cover("id-fixture:1", 9).await.unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    // An entry with no cover on this device, another provider's entry and an unknown one.
    let status = fixture.read_cover("id-fixture:2", 0).await.unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    for id in ["id-steam:620", "id-nope"] {
        let status = fixture.read_cover(id, 0).await.unwrap_err();
        assert_eq!(status.code(), Code::NotFound, "{id}");
    }
    let status = fixture
        .client()
        .artwork_read(ArtworkReadRequest {
            context: Some(fixture.context()),
            installation_id: "id-fixture:1".into(),
            artwork_kind: "screenshot".into(),
            ..ArtworkReadRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn the_cover_a_plugin_set_is_what_it_reads_back_with_its_revision() {
    let fixture = Fixture::new(&[cap::ARTWORK_READ, cap::ARTWORK_WRITE]).await;
    fixture.library(SHARED);
    fixture.with_host();
    fixture
        .set_cover("k", 0, "id-fixture:1", png(300, 300), "image/png")
        .await
        .unwrap();
    let read = fixture.read_cover("id-fixture:1", 0).await.unwrap();
    assert_eq!((read.media_type.as_str(), read.revision), ("image/jpeg", 1));
    let decoded = image::load_from_memory(&read.encoded_image).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (300, 300));
}
