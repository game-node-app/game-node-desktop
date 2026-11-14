//! ResourceHost library reads over the real local socket, with the SDK as the client.

use super::*;
use crate::plugin_host::library::{
    CommitGuard, FavoriteWrite, InstallState, LibraryHost, LibrarySource,
};
use crate::plugin_host::provider::{HostServices, Lease, Registration, RegistrationOrigin};
use gamenode_core::library::DisplayWrite;
use gamenode_plugin_sdk::{HostClient, Launch, Session, SessionOptions};
use resource_host_client::ResourceHostClient;
use resource_host_server::ResourceHost;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
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
                    display_name: None,
                    metadata_revision: 0,
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
        .matching_search(MatchingSearchRequest::default())
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
            ("LibraryWatch", 0),
            ("MetadataUpdate", 0),
            ("FavoritesList", MAX_PAGE_SIZE as u32),
            ("FavoritesSet", 0),
        ]
    );
}

// ---- Display metadata and favorites (task 5.2) ----

/// An in-memory engine side: display overrides by entry key and favorite state by
/// `(provider, game)`. A game is "matched" for a provider when it has a row here.
#[derive(Default)]
struct FakeHost {
    names: Mutex<HashMap<String, (Option<String>, u64)>>,
    games: Mutex<BTreeMap<(String, u64), bool>>,
    /// State-changing writes that reached the host.
    writes: AtomicUsize,
    /// Revokes a permission right before the commit guard runs, as a race would.
    revoke_before_commit: Mutex<Option<(ProviderApi, &'static str)>>,
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
