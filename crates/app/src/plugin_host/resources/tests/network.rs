//! `NetworkFetch` over the real local socket (task 6.3). The fetch itself is a scripted stand-in;
//! the address, redirect, size and deadline rules are tested with the real client in
//! `gamenode_platform::plugin_network`, and the real client is used here for the destinations it
//! must refuse.

use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicUsize;

use async_trait::async_trait;
use gamenode_platform::plugin_network::{FetchError, Request as Fetch, Response as Fetched};
use gamenode_plugin_protocol::prost::Message;

use super::*;
use crate::plugin_host::grants::PluginIdentity;
use crate::plugin_host::network::{AllowOrigin, Fetcher, OriginScopes};

const NETWORK: &[&str] = &[cap::NETWORK_FETCH];

/// What the fake saw of one request.
#[derive(Clone, Debug)]
pub(super) struct Seen {
    pub(super) url: String,
    pub(super) method: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
    pub(super) max_response_bytes: usize,
    pub(super) timeout: Duration,
}

type Script = Arc<dyn Fn(&Seen, &AllowOrigin) -> Result<Fetched, FetchError> + Send + Sync>;

#[derive(Default)]
pub(super) struct FakeFetcher {
    calls: StdMutex<Vec<Seen>>,
    script: StdMutex<Option<Script>>,
    gate: StdMutex<Option<Arc<tokio::sync::Semaphore>>>,
    started: AtomicUsize,
}

#[async_trait]
impl Fetcher for FakeFetcher {
    async fn fetch(&self, request: Fetch, allow: AllowOrigin) -> Result<Fetched, FetchError> {
        let seen = Seen {
            url: request.url().to_string(),
            method: request.method().to_owned(),
            headers: request.headers().to_vec(),
            body: request.body().to_vec(),
            max_response_bytes: request.max_response_bytes(),
            timeout: request.timeout(),
        };
        self.calls.lock().unwrap().push(seen.clone());
        self.started.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.acquire().await.unwrap().forget();
        }
        let script = self.script.lock().unwrap().clone();
        match script {
            Some(script) => script(&seen, &allow),
            None => {
                let origin = request.origin().unwrap();
                if !allow(&origin) {
                    return Err(FetchError::Denied);
                }
                Ok(Fetched {
                    status: 200,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    body: b"fetched".to_vec(),
                    final_origin: origin,
                })
            }
        }
    }
}

impl FakeFetcher {
    pub(super) fn calls(&self) -> Vec<Seen> {
        self.calls.lock().unwrap().clone()
    }

    pub(super) fn script(
        &self,
        script: impl Fn(&Seen, &AllowOrigin) -> Result<Fetched, FetchError> + Send + Sync + 'static,
    ) {
        *self.script.lock().unwrap() = Some(Arc::new(script));
    }
}

/// The origins the user approved, changeable between calls.
#[derive(Default)]
pub(super) struct Origins {
    approved: StdMutex<Vec<String>>,
    asked_for: StdMutex<Vec<PluginIdentity>>,
}

impl OriginScopes for Origins {
    fn approved(&self, identity: &PluginIdentity) -> Vec<String> {
        self.asked_for.lock().unwrap().push(identity.clone());
        self.approved.lock().unwrap().clone()
    }
}

impl Origins {
    fn set(&self, origins: &[&str]) {
        *self.approved.lock().unwrap() = origins.iter().map(|o| (*o).to_owned()).collect();
    }
}

impl Fixture {
    pub(super) fn user_approving_origins(&self, origins: &[&str]) -> Arc<Origins> {
        let scopes = Arc::new(Origins::default());
        scopes.set(origins);
        self.network.set_scopes(scopes.clone());
        scopes
    }

    pub(super) async fn fetch_with(
        &self,
        url: &str,
        method: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: u64,
        timeout_ms: u32,
    ) -> Result<NetworkFetchReply, Status> {
        self.client()
            .network_fetch(NetworkFetchRequest {
                context: Some(self.context()),
                url: url.into(),
                method: method.into(),
                headers: headers
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                    .collect(),
                body: body.to_vec(),
                max_response_bytes,
                timeout_ms,
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn fetch_url_text(&self, url: &str) -> Result<NetworkFetchReply, Status> {
        self.fetch_with(url, "", &[], b"", 0, 0).await
    }
}

fn denial(status: &Status) -> DenialDetail {
    DenialDetail::decode(status.details()).unwrap()
}

#[tokio::test]
async fn an_approved_origin_is_fetched_and_the_answer_comes_back() {
    let fixture = Fixture::new(NETWORK).await;
    let scopes = fixture.user_approving_origins(&["https://api.example.com"]);

    let reply = fixture
        .fetch_with(
            "https://api.example.com/v1/items?limit=3#frag",
            "POST",
            &[("Content-Type", "application/json"), ("X-Token", "t")],
            b"{\"a\":1}",
            2048,
            1500,
        )
        .await
        .unwrap();
    assert_eq!(reply.status_code, 200);
    assert_eq!(reply.body, b"fetched");
    assert_eq!(reply.final_origin, "https://api.example.com");
    assert_eq!(reply.headers["content-type"], "text/plain");

    // Exactly what the plugin asked for reached the fetch: nothing of GameNode's added.
    let [seen] = fixture.fetcher.calls().try_into().unwrap();
    assert_eq!(seen.url, "https://api.example.com/v1/items?limit=3");
    assert_eq!(seen.method, "POST");
    let mut headers = seen.headers.clone();
    headers.sort();
    assert_eq!(
        headers,
        [
            ("content-type".to_owned(), "application/json".to_owned()),
            ("x-token".to_owned(), "t".to_owned())
        ]
    );
    assert_eq!(seen.body, b"{\"a\":1}");
    assert_eq!(
        (seen.max_response_bytes, seen.timeout),
        (2048, Duration::from_millis(1500))
    );
    // The origins were asked for the plugin's authenticated identity, not a name it sent.
    let asked = scopes.asked_for.lock().unwrap();
    assert!(
        asked
            .iter()
            .all(|identity| identity.plugin_id == "fixture.plugin"
                && identity.publisher_fingerprint == "ab".repeat(32))
    );
    assert!(!asked.is_empty());
}

#[tokio::test]
async fn the_defaults_are_a_get_with_the_default_budget_and_the_longest_deadline() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    fixture
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap();
    fixture
        .fetch_with("https://api.example.com/", "", &[], b"", u64::MAX, u32::MAX)
        .await
        .unwrap();
    let calls = fixture.fetcher.calls();
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[0].max_response_bytes, 1024 * 1024);
    assert_eq!(calls[0].timeout, Duration::from_secs(30));
    assert_eq!(
        calls[1].max_response_bytes,
        8 * 1024 * 1024,
        "capped at 8 MiB"
    );
    assert_eq!(calls[1].timeout, Duration::from_secs(30), "capped at 30 s");
}

#[tokio::test]
async fn nothing_is_reachable_without_the_grant_and_an_approved_origin() {
    // No grant at all.
    let none = Fixture::new(&[]).await;
    none.user_approving_origins(&["https://api.example.com"]);
    let status = none
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(denial(&status).permission_id, cap::NETWORK_FETCH);

    // The grant, but no origin approved (nothing is connected, as in a fresh install).
    let fixture = Fixture::new(NETWORK).await;
    let status = fixture
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(denial(&status).permission_id, cap::NETWORK_FETCH);
    assert_eq!(denial(&status).reason, DenialReason::ScopeDenied as i32);
    // The refusal is audited like any other.
    assert!(fixture.api.authorization_audit().iter().any(|event| {
        event.permission_id == cap::NETWORK_FETCH && event.reason == DenialReason::ScopeDenied
    }));
    assert!(fixture.fetcher.calls().is_empty(), "nothing was fetched");
}

#[tokio::test]
async fn an_origin_is_exact_scheme_host_and_port() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&[
        "https://api.example.com",
        // Entries that are not exact origins approve nothing.
        "https://other.example.com/v1",
        "http://insecure.example.com",
        "*.example.org",
        "https://*.example.net",
        "",
    ]);
    for url in [
        "https://example.com/",
        "https://www.api.example.com/",
        "https://api.example.com.evil.test/",
        "https://evilapi.example.com/",
        "https://api.example.com:8443/",
        "https://other.example.com/v1",
        "https://insecure.example.com/",
        "https://sub.example.org/",
        "https://sub.example.net/",
        "https://api.example.com@evil.test/",
    ] {
        let status = fixture.fetch_url_text(url).await.unwrap_err();
        assert!(
            matches!(
                status.code(),
                Code::PermissionDenied | Code::InvalidArgument
            ),
            "{url}: {status:?}"
        );
    }
    assert!(fixture.fetcher.calls().is_empty());
    // The approved one, in any spelling of the same origin, is fine.
    for url in [
        "https://api.example.com",
        "https://API.example.com/x",
        "https://api.example.com:443/x",
    ] {
        fixture.fetch_url_text(url).await.unwrap();
    }
}

#[tokio::test]
async fn widening_or_revoking_an_origin_takes_effect_on_the_next_call_and_at_every_redirect() {
    let fixture = Fixture::new(NETWORK).await;
    let scopes = fixture.user_approving_origins(&["https://api.example.com"]);
    fixture
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap();
    assert_eq!(
        fixture
            .fetch_url_text("https://cdn.example.com/")
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    scopes.set(&["https://api.example.com", "https://cdn.example.com"]);
    fixture
        .fetch_url_text("https://cdn.example.com/")
        .await
        .unwrap();
    scopes.set(&[]);
    assert_eq!(
        fixture
            .fetch_url_text("https://api.example.com/")
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );

    // A redirect to an origin that is not approved is refused where the fetch asks.
    scopes.set(&["https://api.example.com"]);
    let hops = Arc::new(StdMutex::new(Vec::new()));
    let recorded = hops.clone();
    fixture.fetcher.script(move |_, allow| {
        for origin in ["https://api.example.com", "https://evil.example.com"] {
            recorded.lock().unwrap().push((origin, allow(origin)));
            if !allow(origin) {
                return Err(FetchError::Denied);
            }
        }
        unreachable!("the unapproved origin must stop the fetch")
    });
    let status = fixture
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(denial(&status).reason, DenialReason::ScopeDenied as i32);
    assert_eq!(
        *hops.lock().unwrap(),
        [
            ("https://api.example.com", true),
            ("https://evil.example.com", false)
        ]
    );
    // The refusal says nothing about where the redirect pointed.
    assert!(!format!("{status:?}").contains("evil.example.com"));
}

#[tokio::test]
async fn a_grant_revoked_while_a_fetch_runs_stops_its_next_hop() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    let api = fixture.api.clone();
    fixture.fetcher.script(move |_, allow| {
        assert!(allow("https://api.example.com"));
        // The permission is revoked before the redirect is followed.
        let runtime = tokio::runtime::Handle::current();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    runtime.block_on(api.revoke_session_permission("fixture", cap::NETWORK_FETCH))
                })
                .join()
                .unwrap();
        });
        if allow("https://api.example.com") {
            return Ok(Fetched {
                status: 200,
                headers: vec![],
                body: b"leaked".to_vec(),
                final_origin: "https://api.example.com".into(),
            });
        }
        Err(FetchError::Denied)
    });
    let status = fixture
        .fetch_url_text("https://api.example.com/")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn requests_are_validated_before_anything_is_fetched() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    let long_url = format!("https://api.example.com/{}", "a".repeat(2100));
    let big_body = vec![0u8; 1024 * 1024 + 1];
    let long_value = "v".repeat(5000);
    type Case<'a> = (String, &'a str, Vec<(&'a str, &'a str)>, Vec<u8>);
    let cases: Vec<Case> = vec![
        ("not a url".into(), "GET", vec![], vec![]),
        ("http://api.example.com/".into(), "GET", vec![], vec![]),
        (
            "https://user:pw@api.example.com/".into(),
            "GET",
            vec![],
            vec![],
        ),
        (long_url, "GET", vec![], vec![]),
        ("https://api.example.com/".into(), "CONNECT", vec![], vec![]),
        ("https://api.example.com/".into(), "get", vec![], vec![]),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("Host", "evil.test")],
            vec![],
        ),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("Content-Length", "1")],
            vec![],
        ),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("X-A", "line\r\nInjected: 1")],
            vec![],
        ),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("X-A", &long_value)],
            vec![],
        ),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("bad name", "x")],
            vec![],
        ),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![("X-A", "1"), ("x-a", "2")],
            vec![],
        ),
        ("https://api.example.com/".into(), "POST", vec![], big_body),
        (
            "https://api.example.com/".into(),
            "GET",
            vec![],
            b"body".to_vec(),
        ),
    ];
    for (url, method, headers, body) in cases {
        let shown = format!("{method} {:.60}", url);
        let status = fixture
            .fetch_with(&url, method, &headers, &body, 0, 0)
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{shown}: {status:?}");
    }
    assert!(fixture.fetcher.calls().is_empty());
}

#[tokio::test]
async fn failures_map_to_distinct_codes_and_never_say_where_the_destination_is() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    for (error, code) in [
        (FetchError::TooLarge, Code::ResourceExhausted),
        (FetchError::Timeout, Code::DeadlineExceeded),
        (FetchError::TooManyRedirects, Code::FailedPrecondition),
        (FetchError::Unreachable, Code::Unavailable),
        (FetchError::BadResponse, Code::Unavailable),
        (FetchError::Denied, Code::PermissionDenied),
    ] {
        fixture.fetcher.script(move |_, _| Err(error));
        let status = fixture
            .fetch_url_text("https://api.example.com/")
            .await
            .unwrap_err();
        assert_eq!(status.code(), code, "{error:?}");
        let shown = format!("{status:?}");
        assert!(
            !shown.contains("api.example.com") && !shown.contains("127.0."),
            "{shown}"
        );
    }
}

#[tokio::test]
async fn a_plugin_may_have_only_a_few_fetches_running() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *fixture.fetcher.gate.lock().unwrap() = Some(gate.clone());

    let running: Vec<_> = (0..crate::plugin_host::network::PER_PLUGIN_FETCHES)
        .map(|index| {
            let mut client = fixture.client();
            let request = NetworkFetchRequest {
                context: Some(fixture.context()),
                url: format!("https://api.example.com/{index}"),
                ..Default::default()
            };
            tokio::spawn(async move { client.network_fetch(request).await })
        })
        .collect();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while fixture.fetcher.started.load(Ordering::SeqCst) < running.len() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fetches never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // One more is told to retry; it never reaches the fetch.
    let status = fixture
        .fetch_url_text("https://api.example.com/extra")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(fixture.fetcher.calls().len(), running.len());

    gate.add_permits(100);
    for task in running {
        task.await.unwrap().unwrap();
    }
    // The slots are free again.
    fixture
        .fetch_url_text("https://api.example.com/after")
        .await
        .unwrap();
}

#[tokio::test]
async fn the_real_client_refuses_local_and_private_destinations_before_connecting() {
    let fixture = Fixture::new(NETWORK).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    });

    let urls = [
        format!("https://127.0.0.1:{port}/"),
        format!("https://localhost:{port}/"),
        format!("https://[::1]:{port}/"),
        format!("https://0x7f.1:{port}/"),
        format!("https://2130706433:{port}/"),
        "https://10.0.0.5/".to_owned(),
        "https://192.168.1.1/".to_owned(),
        "https://172.16.0.1/".to_owned(),
        "https://169.254.169.254/latest/meta-data/".to_owned(),
        "https://100.64.0.1/".to_owned(),
        "https://[fd00::1]/".to_owned(),
        "https://[::ffff:127.0.0.1]/".to_owned(),
        "https://0.0.0.0/".to_owned(),
    ];
    // The user approved every one of them: an approved origin is still not enough.
    let origins: Vec<String> = urls
        .iter()
        .map(|url| url::Url::parse(url).unwrap().origin().ascii_serialization())
        .collect();
    let service = ResourceApi::new(
        fixture.api.clone(),
        fixture.feed.clone(),
        fixture.storage.clone(),
        fixture.files.clone(),
        Arc::new(NetworkAccess::default()),
        fixture.notifications.clone(),
        fixture.jobs.clone(),
        fixture.intents.clone(),
        Arc::default(),
    );
    let scopes = Arc::new(Origins::default());
    *scopes.approved.lock().unwrap() = origins;
    service.network.set_scopes(scopes);
    for url in &urls {
        let status = service
            .network_fetch(Request::new(NetworkFetchRequest {
                context: Some(fixture.context()),
                url: url.clone(),
                timeout_ms: 5000,
                ..Default::default()
            }))
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::PermissionDenied, "{url}: {status:?}");
        let shown = format!("{status:?}");
        assert!(
            !shown.contains("127.0.0.1") && !shown.contains("169.254"),
            "{shown}"
        );
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        0,
        "no connection reached a local server"
    );
}

#[tokio::test]
async fn the_network_service_is_described_and_needs_its_own_grant() {
    let fixture = Fixture::new(NETWORK).await;
    let described = fixture.host.describe_api().await.unwrap();
    let method = described
        .methods
        .iter()
        .find(|method| method.method == "NetworkFetch")
        .expect("NetworkFetch is served");
    assert!(
        method
            .permissions
            .iter()
            .any(|permission| permission == cap::NETWORK_FETCH)
    );
    // Another grant does not stand in for it.
    let other = Fixture::new(&[cap::FILES_READ, cap::STORAGE_READ]).await;
    other.user_approving_origins(&["https://api.example.com"]);
    assert_eq!(
        other
            .fetch_url_text("https://api.example.com/")
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}
