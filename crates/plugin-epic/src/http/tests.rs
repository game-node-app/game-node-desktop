use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gamenode_plugin_protocol::models::SessionCredential;
use httpmock::prelude::*;
use tracing::field::{Field, Visit};
use tracing::span;
use tracing::{Event, Metadata, Subscriber};
use url::Url;

use super::*;

const BASIC_AUTH: &str =
    "MzRhMDJjZjhmNDQxNGUyOWIxNTkyMTg3NmRhMzZmOWE6ZGFhZmJjY2M3Mzc3NDUwMzlkZmZlNTNkOTRmYzc2Y2Y=";
const CODE: &str = "authorization-code-secret";
const ACCESS: &str = "access-token-secret";
const REFRESH: &str = "refresh-token-secret";
const ROTATED_REFRESH: &str = "rotated-refresh-token-secret";
const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const NAMESPACE: &str = "0123456789abcdef0123456789abcdef";

fn client(server: &MockServer) -> EpicClient {
    let base = Url::parse(&server.base_url()).unwrap();
    EpicClient::with_bases(base.clone(), base.clone(), base, Duration::from_secs(2)).unwrap()
}

fn cred(value: &str) -> SessionCredential {
    SessionCredential::new(value).unwrap()
}

#[tokio::test]
async fn authorization_code_exchange_returns_the_account_identity() {
    let server = MockServer::start_async().await;
    let exchange = server.mock(|when, then| {
        when.method(POST)
            .path(TOKEN_PATH)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("authorization", format!("Basic {BASIC_AUTH}"))
            .body_includes("grant_type=authorization_code")
            .body_includes("code=authorization-code-secret")
            .body_includes("token_type=eg1");
        then.status(200)
            .body(include_str!("fixtures/token_success.json"));
    });

    let token = client(&server).exchange_code(&cred(CODE)).await.unwrap();

    exchange.assert();
    assert_eq!(token.access_token.expose(), ACCESS);
    assert_eq!(token.refresh_token.expose(), REFRESH);
    assert_eq!(token.account_id, ACCOUNT);
    assert_eq!(token.display_name, "Player One");
}

#[tokio::test]
async fn refresh_grant_returns_the_rotated_refresh_token() {
    let server = MockServer::start_async().await;
    let exchange = server.mock(|when, then| {
        when.method(POST)
            .path(TOKEN_PATH)
            .body_includes("grant_type=refresh_token")
            .body_includes("refresh_token=refresh-token-secret")
            .body_includes("token_type=eg1");
        then.status(200)
            .body(include_str!("fixtures/token_rotated.json"));
    });

    let token = client(&server).refresh(&cred(REFRESH)).await.unwrap();

    exchange.assert();
    assert_eq!(token.access_token.expose(), "rotated-access-token-secret");
    assert_eq!(token.refresh_token.expose(), ROTATED_REFRESH);
}

#[tokio::test]
async fn invalid_grant_is_a_secret_free_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(POST).path(TOKEN_PATH);
        then.status(400)
            .body(include_str!("fixtures/oauth_invalid_grant.json"));
    });

    let error = client(&server)
        .exchange_code(&cred(CODE))
        .await
        .unwrap_err();
    assert_eq!(error, EpicError::InvalidGrant);
    assert!(!format!("{error}").contains(CODE));
}

#[tokio::test]
async fn an_unauthorized_refresh_is_its_own_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(POST).path(TOKEN_PATH);
        then.status(401).body(r#"{"errorCode":"unauthorized"}"#);
    });

    let error = client(&server).refresh(&cred(REFRESH)).await.unwrap_err();
    assert_eq!(error, EpicError::Unauthorized);
}

#[tokio::test]
async fn a_rate_limited_library_page_is_its_own_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path(LIBRARY_ITEMS_PATH);
        then.status(429);
    });

    let error = client(&server)
        .library_page(&cred(ACCESS), None)
        .await
        .unwrap_err();
    assert_eq!(error, EpicError::RateLimited);
}

#[tokio::test]
async fn a_catalog_server_error_is_its_own_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path(format!(
            "/catalog/api/shared/namespace/{NAMESPACE}/bulk/items"
        ));
        then.status(503);
    });

    let error = client(&server)
        .catalog_items(&cred(ACCESS), NAMESPACE, &["catalog-alpha".to_owned()])
        .await
        .unwrap_err();
    assert_eq!(error, EpicError::Server);
}

#[tokio::test]
async fn a_malformed_token_body_is_a_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(POST).path(TOKEN_PATH);
        then.status(200)
            .body(r#"{"access_token": "not enough fields"}"#);
    });

    let error = client(&server)
        .exchange_code(&cred(CODE))
        .await
        .unwrap_err();
    assert_eq!(error, EpicError::Changed);
}

#[tokio::test]
async fn a_malformed_library_body_is_a_category() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path(LIBRARY_ITEMS_PATH);
        then.status(200).body("not json at all");
    });

    let error = client(&server)
        .library_page(&cred(ACCESS), None)
        .await
        .unwrap_err();
    assert_eq!(error, EpicError::Changed);
}

#[tokio::test]
async fn library_pages_follow_the_cursor() {
    let server = MockServer::start_async().await;
    let first = server.mock(|when, then| {
        when.method(GET)
            .path(LIBRARY_ITEMS_PATH)
            .query_param("includeMetadata", "true")
            .query_param_missing("cursor")
            .header("authorization", format!("Bearer {ACCESS}"));
        then.status(200)
            .body(include_str!("fixtures/library_page_one.json"));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path(LIBRARY_ITEMS_PATH)
            .query_param("includeMetadata", "true")
            .query_param("cursor", "cursor-page-2")
            .header("authorization", format!("Bearer {ACCESS}"));
        then.status(200)
            .body(include_str!("fixtures/library_page_two.json"));
    });

    let items = client(&server).library_items(&cred(ACCESS)).await.unwrap();

    first.assert_calls(1);
    second.assert_calls(1);
    assert_eq!(
        items
            .iter()
            .map(|item| item.app_name.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["GameAlpha", "GameBeta", "GameGamma"]
    );
    assert_eq!(items[0].namespace.as_deref(), Some(NAMESPACE));
    assert_eq!(items[0].catalog_item_id.as_deref(), Some("catalog-alpha"));
    assert_eq!(items[0].artifact_id.as_deref(), Some("artifact-alpha"));
    assert_eq!(items[1].artifact_id.as_deref(), Some("artifact-beta"));
    assert_eq!(items[2].artifact_id.as_deref(), Some("artifact-gamma"));
}

#[tokio::test]
async fn catalog_bulk_parses_titles_and_game_flags() {
    let server = MockServer::start_async().await;
    let bulk = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/catalog/api/shared/namespace/{NAMESPACE}/bulk/items"
            ))
            .query_param("id", "catalog-alpha,catalog-beta")
            .query_param("includeMainGameDetails", "true")
            .query_param("country", "US")
            .query_param("locale", "en-US")
            .header("authorization", format!("Bearer {ACCESS}"));
        then.status(200)
            .body(include_str!("fixtures/catalog_bulk.json"));
    });

    let items = client(&server)
        .catalog_items(
            &cred(ACCESS),
            NAMESPACE,
            &["catalog-alpha".to_owned(), "catalog-beta".to_owned()],
        )
        .await
        .unwrap();

    bulk.assert();
    let alpha = items.get("catalog-alpha").unwrap();
    assert_eq!(alpha.title, "Alpha Adventure");
    assert!(alpha.is_game);
    assert!(!alpha.has_main_game);
    let beta = items.get("catalog-beta").unwrap();
    assert!(!beta.is_game);
    assert!(beta.has_main_game);
}

#[tokio::test]
async fn playtime_returns_the_cumulative_totals() {
    let server = MockServer::start_async().await;
    let ledger = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/library/api/public/playtime/account/{ACCOUNT}/all"
            ))
            .header("authorization", format!("Bearer {ACCESS}"));
        then.status(200)
            .body(include_str!("fixtures/playtime.json"));
    });

    let totals = client(&server)
        .playtime(&cred(ACCESS), ACCOUNT)
        .await
        .unwrap();

    ledger.assert();
    assert_eq!(totals.len(), 3);
    assert_eq!(totals[0].artifact_id, "artifact-alpha");
    assert_eq!(totals[0].seconds, 7200);
}

#[tokio::test]
async fn revoke_uses_the_session_delete_endpoint() {
    let server = MockServer::start_async().await;
    let revoke = server.mock(|when, then| {
        when.method(DELETE)
            .path(format!("/account/api/oauth/sessions/kill/{ACCESS}"))
            .header("authorization", format!("Bearer {ACCESS}"));
        then.status(204);
    });

    client(&server).revoke(&cred(ACCESS)).await.unwrap();

    revoke.assert();
}

#[derive(Default)]
struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push_str(&format!("{}={value:?} ", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push_str(&format!("{}={value} ", field.name()));
    }
}

struct CaptureSubscriber {
    events: Arc<Mutex<Vec<String>>>,
    next_span: AtomicU64,
}

impl CaptureSubscriber {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            next_span: AtomicU64::new(1),
        }
    }
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target().starts_with("gamenode_plugin_epic::http")
    }

    fn new_span(&self, _attributes: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if let Ok(mut events) = self.events.lock() {
            events.push(fields.0);
        }
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

#[test]
fn errors_and_logs_never_contain_a_code_token_account_id_or_url() {
    // A process-wide subscriber audits diagnostics across async tasks and concurrent tests.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let subscriber = CaptureSubscriber::new();
    let events = subscriber.events.clone();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let (token_error, library_error, base_url) = {
        runtime.block_on(async {
            let server = MockServer::start_async().await;
            server.mock(|when, then| {
                when.method(POST).path(TOKEN_PATH);
                then.status(400)
                    .body(format!("SECRET_BODY {CODE} {ACCESS} {REFRESH} {ACCOUNT}"));
            });
            server.mock(|when, then| {
                when.method(GET).path(LIBRARY_ITEMS_PATH);
                then.status(500)
                    .body(format!("SECRET_BODY {CODE} {ACCESS} {REFRESH} {ACCOUNT}"));
            });

            let client = client(&server);
            let token_error = client.exchange_code(&cred(CODE)).await.unwrap_err();
            let library_error = client
                .library_page(&cred(ACCESS), Some("cursor-secret"))
                .await
                .unwrap_err();
            (token_error, library_error, server.base_url())
        })
    };
    assert_eq!(token_error, EpicError::Rejected);
    assert_eq!(library_error, EpicError::Server);

    let token = EpicToken {
        access_token: cred(ACCESS),
        refresh_token: cred(REFRESH),
        account_id: ACCOUNT.to_owned(),
        display_name: "Player One".to_owned(),
    };
    let logs = events.lock().unwrap().join("\n");
    for secret in [
        CODE,
        ACCESS,
        REFRESH,
        ACCOUNT,
        "SECRET_BODY",
        "cursor-secret",
    ] {
        assert!(!logs.contains(secret), "a log line leaked {secret}: {logs}");
        for error in [token_error, library_error] {
            assert!(!format!("{error}").contains(secret));
            assert!(!format!("{error:?}").contains(secret));
        }
        assert!(!format!("{token:?}").contains(secret));
    }
    assert!(!logs.contains(&base_url));
    assert!(!logs.contains("http://"));
    assert!(
        logs.contains("oauth_token") && logs.contains("library_items"),
        "logs name only the endpoint: {logs}"
    );
}
