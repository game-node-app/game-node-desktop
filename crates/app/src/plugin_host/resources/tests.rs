//! ResourceHost library reads over the real local socket, with the SDK as the client.

use super::*;
use crate::plugin_host::library::{InstallState, LibrarySource};
use crate::plugin_host::provider::{HostServices, Lease, Registration, RegistrationOrigin};
use gamenode_plugin_sdk::{HostClient, Launch, Session, SessionOptions};
use resource_host_client::ResourceHostClient;
use resource_host_server::ResourceHost;
use tonic::Code;
use tonic::transport::Channel;

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
        let temp = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = temp.path().join("plugins.sock");
        let root = crate::plugin_host::PluginApi::default();
        let api = root.providers.clone();
        let feed = root.library.clone();
        api.profiles().update(Some("a"), true);
        let mut capabilities = vec![cap::CONTEXT];
        capabilities.extend_from_slice(grants);
        api.register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "ab".repeat(32),
            },
            capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
            shared_catalog: false,
            services: Arc::new(NoServices),
        })
        .unwrap();
        let launch = api.begin_launch("fixture").unwrap();
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
            endpoint,
            credential,
            operation_id: "launch".into(),
        };
        let options = if api_1_6 {
            SessionOptions {
                min_api_major: 1,
                min_api_minor: cap::THIRD_PARTY_API_MINOR,
                max_api_major: 1,
                max_api_minor: gamenode_plugin_protocol::API_MINOR,
                ..SessionOptions::default()
            }
        } else {
            SessionOptions::default()
        };
        let session = Session::open_with_options(
            client_launch,
            "fixture.plugin",
            "fixture",
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
    let service = ResourceApi::new(fixture.api.clone(), fixture.feed.clone());
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
async fn unserved_methods_are_unimplemented_and_describe_api_lists_the_served_ones() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let status = fixture
        .client()
        .metadata_update(MetadataUpdateRequest::default())
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
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
            ("LibraryList", MAX_PAGE_SIZE as u32),
            ("LibraryGet", 0),
            ("LibraryWatch", 0)
        ]
    );
}
