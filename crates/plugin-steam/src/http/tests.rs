use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::steam::{SteamError, SteamOwnedGame, SteamSession, SteamWebApi};
use gamenode_plugin_protocol::models::SessionCredential;
use httpmock::prelude::*;
use serde_json::json;
use tracing::field::{Field, Visit};
use tracing::span;
use tracing::{Event, Metadata, Subscriber};
use url::Url;

use super::{OWNED_GAMES_PATH, SteamClient};

const STEAM_ID: &str = "76561198000000001";
const TOKEN: &str = "webapi-token-secret";
const COOKIE: &str =
    "steamLoginSecure=76561198000000001%7C%7Ccookie-secret; sessionid=session-secret";
const FALLBACK_PATH: &str = "/pointssummary/ajaxgetasyncconfig";
const FAMILY_GROUP_PATH: &str = "/IFamilyGroupsService/GetFamilyGroupForUser/v1/";
const SHARED_LIBRARY_PATH: &str = "/IFamilyGroupsService/GetSharedLibraryApps/v1/";

fn client(server: &MockServer) -> SteamClient {
    let base = Url::parse(&server.base_url()).unwrap();
    SteamClient::with_bases(base.clone(), base, Duration::from_secs(2)).unwrap()
}

fn cred(value: &str) -> SessionCredential {
    SessionCredential::new(value).unwrap()
}

fn session() -> SteamSession {
    SteamSession {
        steam_id: STEAM_ID.to_owned(),
        account_name: Some("Player One".to_owned()),
        token: cred(TOKEN),
        cookie: cred(COOKIE),
    }
}

fn page(userinfo: &str, config: &str) -> String {
    format!(
        "<html><body><div id=\"application_config\" data-userinfo=\"{userinfo}\" \
         data-store_user_config=\"{config}\"></div></body></html>"
    )
}

fn logged_in_page() -> String {
    page(
        "{&quot;logged_in&quot;:true,&quot;steamid&quot;:&quot;76561198000000001&quot;,\
         &quot;account_name&quot;:&quot;Player One&quot;}",
        "{&quot;webapi_token&quot;:&quot;webapi-token-secret&quot;}",
    )
}

/// Steam's store front compresses some responses even when the client sent no `Accept-Encoding`;
/// a browser (and Playnite's WebView) decodes them.
fn gzip(body: &str) -> Vec<u8> {
    use std::io::Write;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(body.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

fn exact_params(
    expected: &[(&str, &str)],
) -> impl Fn(&HttpMockRequest) -> bool + Send + Sync + 'static {
    let mut expected: Vec<(String, String)> = expected
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    expected.sort();
    move |request| {
        let mut actual = request.query_params();
        actual.sort();
        actual == expected
    }
}

fn owned_params() -> Vec<(&'static str, &'static str)> {
    vec![
        ("access_token", TOKEN),
        ("steamid", STEAM_ID),
        ("format", "json"),
        ("language", "english"),
        ("include_appinfo", "true"),
        ("include_played_free_games", "true"),
        ("include_free_sub", "false"),
        ("skip_unvetted_apps", "false"),
    ]
}

fn family_group_params() -> Vec<(&'static str, &'static str)> {
    vec![("access_token", TOKEN), ("steamid", STEAM_ID)]
}

fn shared_params() -> Vec<(&'static str, &'static str)> {
    vec![
        ("access_token", TOKEN),
        ("steamid", STEAM_ID),
        ("family_groupid", "987654"),
        ("language", "english"),
        ("include_own", "false"),
        ("include_excluded", "false"),
        ("include_free", "false"),
        ("include_non_games", "false"),
        ("max_apps", "10000"),
    ]
}

#[tokio::test]
async fn logged_in_store_page_yields_the_session_and_token() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/").header("cookie", COOKIE);
        then.status(200).body(logged_in_page());
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    store.assert_async().await;

    assert_eq!(session.steam_id, STEAM_ID);
    assert_eq!(session.account_name.as_deref(), Some("Player One"));
    assert_eq!(session.token.expose(), TOKEN);
    assert_eq!(session.cookie.expose(), COOKIE);
}

#[tokio::test]
async fn compressed_store_pages_are_decoded_like_a_browser() {
    // The recommendation queue gzips even when the request did not advertise `Accept-Encoding`,
    // exactly like the live endpoint. A client without decompression reports `changed` forever.
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/").header("cookie", COOKIE);
        then.status(200)
            .header("content-type", "text/html; charset=UTF-8")
            .header("content-encoding", "gzip")
            .body(gzip(&logged_in_page()));
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    store.assert_async().await;
    assert_eq!(session.steam_id, STEAM_ID);
    assert_eq!(session.token.expose(), TOKEN);
}

#[tokio::test]
async fn store_page_requests_carry_browser_headers() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET)
            .path("/explore/")
            .header_includes("accept", "text/html")
            .header("accept-language", "en-US,en;q=0.9")
            .header_includes("accept-encoding", "gzip");
        then.status(200).body(logged_in_page());
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    store.assert_async().await;
    assert_eq!(session.steam_id, STEAM_ID);
}

#[tokio::test]
async fn the_home_page_supplies_the_config_when_the_queue_does_not() {
    let server = MockServer::start_async().await;
    let queue = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>a layout this client does not know</body></html>");
    });
    let home = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200).body(logged_in_page());
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    queue.assert_async().await;
    home.assert_async().await;
    assert_eq!(session.steam_id, STEAM_ID);
    assert_eq!(session.token.expose(), TOKEN);
}

#[tokio::test]
async fn logged_out_store_page_is_expired_without_the_fallback() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200).body(page(
            "{&quot;logged_in&quot;:false,&quot;steamid&quot;:&quot;76561198000000001&quot;}",
            "{&quot;webapi_token&quot;:&quot;webapi-token-secret&quot;}",
        ));
    });
    let fallback = server.mock(|when, then| {
        when.method(GET).path(FALLBACK_PATH);
        then.status(200)
            .json_body(json!({"success": 1, "data": {"webapi_token": TOKEN}}));
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Expired);
    store.assert_async().await;
    fallback.assert_calls_async(0).await;
}

#[tokio::test]
async fn rotated_session_cookies_replace_the_stored_values() {
    let server = MockServer::start_async().await;
    let stored = "steamLoginSecure=76561198000000001%7C%7Cold; sessionid=old-session; \
                  steamCountry=US";
    let rotated = "steamLoginSecure=76561198000000001%7C%7Crotated; sessionid=rotated-session; \
                   steamCountry=US";
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/").header("cookie", stored);
        then.status(200)
            .header(
                "set-cookie",
                "steamLoginSecure=76561198000000001%7C%7Crotated; Path=/; Secure; HttpOnly",
            )
            .header("set-cookie", "sessionid=rotated-session; Path=/")
            .body(logged_in_page());
    });

    let session = client(&server).session(&cred(stored)).await.unwrap();
    store.assert_async().await;
    assert_eq!(session.cookie.expose(), rotated);
}

#[tokio::test]
async fn html_entities_are_decoded_before_json_parsing() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200).body(page(
            "{&quot;logged_in&quot;:true,&quot;steamid&quot;:&quot;76561198000000001&quot;,\
             &quot;account_name&quot;:&quot;Rock &amp; Roll&#33;&quot;}",
            "{&quot;webapi_token&quot;:&quot;webapi-token-&#49;23&quot;}",
        ));
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    store.assert_async().await;
    assert_eq!(session.account_name.as_deref(), Some("Rock & Roll!"));
    assert_eq!(session.token.expose(), "webapi-token-123");
}

#[tokio::test]
async fn missing_store_attributes_are_reported_as_changed() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body><div id=\"application_config\"></div></body></html>");
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Changed);
    store.assert_async().await;
}

#[tokio::test]
async fn oversized_store_pages_are_refused() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200).body("x".repeat(4 * 1024 * 1024 + 1));
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Changed);
    store.assert_async().await;
}

#[tokio::test]
async fn a_login_redirect_means_the_session_expired() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(302).header("location", "/login/?redir=store");
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Expired);
    store.assert_async().await;
}

#[tokio::test]
async fn a_cookie_bootstrap_redirect_to_the_same_url_is_followed_with_the_new_cookies() {
    let server = MockServer::start_async().await;
    // Steam answers the first request with Set-Cookie and a redirect back to the same URL, and
    // serves the page once the cookie comes back.
    let page_with_cookie = server.mock(|when, then| {
        when.method(GET)
            .path("/explore/")
            .header_includes("cookie", "browserid=bootstrap")
            .header_includes("cookie", "sessionid=session-secret");
        then.status(200).body(logged_in_page());
    });
    let bootstrap = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(302)
            .header("location", "/explore/")
            .header("set-cookie", "browserid=bootstrap; Path=/; Secure");
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    assert_eq!(session.steam_id, STEAM_ID);
    // The bootstrap cookies are request-local; only the session cookies are persisted.
    assert_eq!(session.cookie.expose(), COOKIE);
    page_with_cookie.assert_calls_async(1).await;
    bootstrap.assert_calls_async(1).await;
}

#[tokio::test]
async fn a_redirect_to_the_same_url_without_new_cookies_is_not_followed_forever() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(302).header("location", "/explore/");
    });
    let home = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(302).header("location", "/");
    });
    let fallback = server.mock(|when, then| {
        when.method(GET).path(FALLBACK_PATH);
        then.status(404);
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Changed);
    store.assert_calls_async(1).await;
    home.assert_calls_async(1).await;
    fallback.assert_calls_async(1).await;
}

#[tokio::test]
async fn the_config_fallback_supplies_the_token_and_identity_from_the_cookie() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>no config here</body></html>");
    });
    let fallback = server.mock(|when, then| {
        when.method(GET)
            .path(FALLBACK_PATH)
            .header("cookie", COOKIE);
        then.status(200)
            .json_body(json!({"success": 1, "data": {"webapi_token": TOKEN}}));
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    store.assert_async().await;
    fallback.assert_async().await;
    assert_eq!(session.steam_id, STEAM_ID);
    assert_eq!(session.account_name, None);
    assert_eq!(session.token.expose(), TOKEN);
    assert_eq!(session.cookie.expose(), COOKIE);
}

#[tokio::test]
async fn a_compressed_token_fallback_is_decoded() {
    let server = MockServer::start_async().await;
    let queue = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>no config here</body></html>");
    });
    let home = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200)
            .body("<html><body>no config here either</body></html>");
    });
    let fallback = server.mock(|when, then| {
        when.method(GET)
            .path(FALLBACK_PATH)
            .header("cookie", COOKIE);
        then.status(200)
            .header("content-encoding", "gzip")
            .body(gzip(
                &serde_json::json!({"success": 1, "data": {"webapi_token": TOKEN}}).to_string(),
            ));
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    queue.assert_async().await;
    home.assert_async().await;
    fallback.assert_async().await;
    assert_eq!(session.steam_id, STEAM_ID);
    assert_eq!(session.token.expose(), TOKEN);
}

#[tokio::test]
async fn a_bootstrap_redirect_on_the_token_fallback_is_replayed() {
    let server = MockServer::start_async().await;
    let queue = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>no config here</body></html>");
    });
    let home = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200)
            .body("<html><body>no config here either</body></html>");
    });
    // The token endpoint bootstraps the same way, and may rotate the session cookie on the bounce.
    let fallback_with_cookie = server.mock(|when, then| {
        when.method(GET)
            .path(FALLBACK_PATH)
            .header_includes("cookie", "browserid=fallback-bootstrap")
            .header_includes("cookie", "steamLoginSecure=76561198000000001%7C%7Crotated");
        then.status(200)
            .json_body(json!({"success": 1, "data": {"webapi_token": TOKEN}}));
    });
    let bootstrap = server.mock(|when, then| {
        when.method(GET).path(FALLBACK_PATH);
        then.status(302)
            .header("location", FALLBACK_PATH)
            .header("set-cookie", "browserid=fallback-bootstrap; Path=/; Secure")
            .header(
                "set-cookie",
                "steamLoginSecure=76561198000000001%7C%7Crotated; Path=/; Secure; HttpOnly",
            );
    });

    let session = client(&server).session(&cred(COOKIE)).await.unwrap();
    queue.assert_async().await;
    home.assert_async().await;
    fallback_with_cookie.assert_async().await;
    bootstrap.assert_async().await;
    assert_eq!(session.token.expose(), TOKEN);
    // The rotation Steam sent on the bounce hop is persisted; the bootstrap cookie is not.
    assert_eq!(
        session.cookie.expose(),
        "steamLoginSecure=76561198000000001%7C%7Crotated; sessionid=session-secret"
    );
}

#[tokio::test]
async fn an_empty_config_fallback_means_expired() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>no config here</body></html>");
    });
    let fallback = server.mock(|when, then| {
        when.method(GET).path(FALLBACK_PATH);
        then.status(200)
            .json_body(json!({"success": 1, "data": {}}));
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Expired);
    store.assert_async().await;
    fallback.assert_async().await;
}

#[tokio::test]
async fn an_empty_array_config_fallback_means_expired() {
    // Steam answers with an empty PHP array, not an object, when it has no token to give.
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(200)
            .body("<html><body>no config here</body></html>");
    });
    let fallback = server.mock(|when, then| {
        when.method(GET).path(FALLBACK_PATH);
        then.status(200)
            .json_body(json!({"success": 1, "data": []}));
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Expired);
    store.assert_async().await;
    fallback.assert_async().await;
}

#[tokio::test]
async fn the_owned_catalog_keeps_numeric_ids_with_clean_names() {
    let server = MockServer::start_async().await;
    let owned = server.mock(|when, then| {
        when.method(GET)
            .path(OWNED_GAMES_PATH)
            .is_true(exact_params(&owned_params()));
        then.status(200).json_body(json!({
            "response": {
                "game_count": 6,
                "games": [
                    {"appid": 620, "name": " Portal 2™ "},
                    {"appid": "730", "name": "Counter-Strike 2®"},
                    {"appid": 620, "name": "Portal 2 duplicate"},
                    {"appid": 999, "name": "   "},
                    {"appid": "not-a-number", "name": "Broken"},
                    {"appid": 400, "name": "Portal ©"}
                ]
            }
        }));
    });

    let games = client(&server).owned_games(&session()).await.unwrap();
    owned.assert_async().await;
    assert_eq!(
        games,
        vec![
            SteamOwnedGame {
                app_id: "620".to_owned(),
                name: "Portal 2".to_owned(),
            },
            SteamOwnedGame {
                app_id: "730".to_owned(),
                name: "Counter-Strike 2".to_owned(),
            },
            SteamOwnedGame {
                app_id: "400".to_owned(),
                name: "Portal".to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn an_empty_owned_library_is_a_successful_empty_catalog() {
    let server = MockServer::start_async().await;
    let owned = server.mock(|when, then| {
        when.method(GET).path(OWNED_GAMES_PATH);
        then.status(200)
            .json_body(json!({"response": {"game_count": 0, "games": []}}));
    });

    let games = client(&server).owned_games(&session()).await.unwrap();
    owned.assert_async().await;
    assert!(games.is_empty());
}

#[tokio::test]
async fn a_private_owned_library_is_reported_as_privacy() {
    for body in [json!({"response": {}}), json!({})] {
        let server = MockServer::start_async().await;
        let owned = server.mock(|when, then| {
            when.method(GET).path(OWNED_GAMES_PATH);
            then.status(200).json_body(body.clone());
        });

        let error = client(&server).owned_games(&session()).await.unwrap_err();
        owned.assert_async().await;
        assert_eq!(error, SteamError::Privacy);
    }
}

#[tokio::test]
async fn rejected_owned_catalogs_are_expired() {
    for status in [401, 403] {
        let server = MockServer::start_async().await;
        let owned = server.mock(|when, then| {
            when.method(GET).path(OWNED_GAMES_PATH);
            then.status(status);
        });

        let error = client(&server).owned_games(&session()).await.unwrap_err();
        owned.assert_async().await;
        assert_eq!(error, SteamError::Expired);
    }
}

#[tokio::test]
async fn a_rate_limited_request_is_retried_and_recovers() {
    let server = MockServer::start_async().await;
    let limited = server.mock(|when, then| {
        when.method(GET).path(OWNED_GAMES_PATH);
        then.status(429).header("retry-after", "0");
    });
    limited.up_to_n_times(1);
    let owned = server.mock(|when, then| {
        when.method(GET).path(OWNED_GAMES_PATH);
        then.status(200)
            .json_body(json!({"response": {"game_count": 1, "games": [
                {"appid": 620, "name": "Portal 2"}
            ]}}));
    });

    let games = client(&server).owned_games(&session()).await.unwrap();
    limited.assert_async().await;
    owned.assert_async().await;
    assert_eq!(games.len(), 1);
}

#[tokio::test]
async fn an_unfollowed_redirect_to_the_sign_in_page_means_expired() {
    let server = MockServer::start_async().await;
    // An http Location on a Steam host is never followed, so only the Location can show it.
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(302).header(
            "location",
            "http://store.steampowered.com/login/?redir=store",
        );
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Expired);
    store.assert_async().await;
}

#[tokio::test]
async fn an_unfollowed_redirect_elsewhere_is_a_server_error() {
    let server = MockServer::start_async().await;
    let store = server.mock(|when, then| {
        when.method(GET).path("/explore/");
        then.status(302)
            .header("location", "https://elsewhere.example/");
    });

    let error = client(&server).session(&cred(COOKIE)).await.unwrap_err();
    assert_eq!(error, SteamError::Changed);
    store.assert_async().await;
}

#[test]
fn redirect_diagnostics_keep_the_host_and_path_but_not_the_query() {
    let base = Url::parse("https://store.steampowered.com/").unwrap();
    let mut headers = reqwest::header::HeaderMap::new();

    headers.insert(
        reqwest::header::LOCATION,
        reqwest::header::HeaderValue::from_static("https://elsewhere.example/a/b?token=secret"),
    );
    assert_eq!(
        super::redirect_target(&base, &headers),
        "elsewhere.example/a/b"
    );

    headers.insert(
        reqwest::header::LOCATION,
        reqwest::header::HeaderValue::from_static("/login/?redir=store"),
    );
    assert_eq!(
        super::redirect_target(&base, &headers),
        "store.steampowered.com/login/"
    );

    assert_eq!(
        super::redirect_target(&base, &reqwest::header::HeaderMap::new()),
        "no usable location"
    );
}

#[tokio::test]
async fn retries_stop_after_three_transient_failures() {
    let server = MockServer::start_async().await;
    let limited = server.mock(|when, then| {
        when.method(GET).path(OWNED_GAMES_PATH);
        then.status(429).header("retry-after", "0");
    });

    let error = client(&server).owned_games(&session()).await.unwrap_err();
    assert_eq!(error, SteamError::RateLimited);
    limited.assert_calls_async(4).await;
}

#[tokio::test]
async fn server_errors_are_retried_then_reported() {
    let server = MockServer::start_async().await;
    let broken = server.mock(|when, then| {
        when.method(GET).path(OWNED_GAMES_PATH);
        then.status(500);
    });

    let error = client(&server).owned_games(&session()).await.unwrap_err();
    assert_eq!(error, SteamError::Server);
    broken.assert_calls_async(4).await;
}

#[tokio::test]
async fn family_games_keep_included_named_apps_and_drop_excluded_ones() {
    let server = MockServer::start_async().await;
    let group = server.mock(|when, then| {
        when.method(GET)
            .path(FAMILY_GROUP_PATH)
            .is_true(exact_params(&family_group_params()));
        then.status(200).json_body(json!({
            "response": {
                "is_not_member_of_any_group": false,
                "family_groupid": "987654"
            }
        }));
    });
    let shared = server.mock(|when, then| {
        when.method(GET)
            .path(SHARED_LIBRARY_PATH)
            .is_true(exact_params(&shared_params()));
        then.status(200).json_body(json!({
            "response": {
                "apps": [
                    {"appid": 10, "name": "Shared One™", "exclude_reason": 0},
                    {"appid": 11, "name": "Excluded", "exclude_reason": 1},
                    {"appid": 10, "name": "Shared duplicate", "exclude_reason": 0},
                    {"appid": "12", "name": "Shared Two", "exclude_reason": 0},
                    {"appid": 13, "name": "  ", "exclude_reason": 0}
                ]
            }
        }));
    });

    let family = client(&server).family_games(&session()).await.unwrap();
    group.assert_async().await;
    shared.assert_async().await;
    assert_eq!(family.group_id.as_deref(), Some("987654"));
    assert_eq!(
        family.games,
        vec![
            SteamOwnedGame {
                app_id: "10".to_owned(),
                name: "Shared One".to_owned(),
            },
            SteamOwnedGame {
                app_id: "12".to_owned(),
                name: "Shared Two".to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn no_family_group_is_a_successful_empty_result() {
    let server = MockServer::start_async().await;
    let group = server.mock(|when, then| {
        when.method(GET).path(FAMILY_GROUP_PATH);
        then.status(200).json_body(json!({
            "response": {"is_not_member_of_any_group": true}
        }));
    });
    let shared = server.mock(|when, then| {
        when.method(GET).path(SHARED_LIBRARY_PATH);
        then.status(200)
            .json_body(json!({"response": {"apps": []}}));
    });

    let family = client(&server).family_games(&session()).await.unwrap();
    group.assert_async().await;
    shared.assert_calls_async(0).await;
    assert_eq!(family.group_id, None);
    assert!(family.games.is_empty());
}

#[tokio::test]
async fn a_family_failure_is_reported_separately() {
    let server = MockServer::start_async().await;
    let group = server.mock(|when, then| {
        when.method(GET).path(FAMILY_GROUP_PATH);
        then.status(404);
    });

    let error = client(&server).family_games(&session()).await.unwrap_err();
    assert_eq!(error, SteamError::Changed);
    group.assert_async().await;
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
        metadata.target().starts_with("gamenode_plugin_steam::http")
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
fn errors_and_logs_never_contain_the_cookie_token_url_or_body() {
    // Audit provider diagnostics across async tasks and concurrent client tests.
    // A process-wide test subscriber avoids thread-local callsite interest
    // races; fixture-server diagnostics are outside this client's log contract.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let subscriber = CaptureSubscriber::new();
    let events = subscriber.events.clone();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let (session_error, owned_error, base_url) = {
        runtime.block_on(async {
            let server = MockServer::start_async().await;
            let store = server.mock(|when, then| {
                when.method(GET).path("/explore/");
                then.status(200).body(format!(
                    "<html><body>SECRET_BODY {TOKEN} {COOKIE}</body></html>"
                ));
            });
            let fallback = server.mock(|when, then| {
                when.method(GET).path(FALLBACK_PATH);
                then.status(404);
            });
            let owned = server.mock(|when, then| {
                when.method(GET).path(OWNED_GAMES_PATH);
                then.status(500);
            });

            let client = client(&server);
            let session_error = client.session(&cred(COOKIE)).await.unwrap_err();
            let owned_error = client.owned_games(&session()).await.unwrap_err();

            store.assert_async().await;
            fallback.assert_async().await;
            owned.assert_calls_async(4).await;
            (session_error, owned_error, server.base_url())
        })
    };
    assert_eq!(session_error, SteamError::Changed);
    assert_eq!(owned_error, SteamError::Server);

    let logs = events.lock().unwrap().join("\n");
    for secret in [
        TOKEN,
        "cookie-secret",
        "session-secret",
        COOKIE,
        "SECRET_BODY",
    ] {
        assert!(!logs.contains(secret), "a log line leaked {secret}: {logs}");
        for error in [session_error, owned_error] {
            assert!(!format!("{error}").contains(secret));
            assert!(!format!("{error:?}").contains(secret));
        }
        assert!(!format!("{:?}", session()).contains(secret));
    }
    assert!(!logs.contains(&base_url));
    assert!(!logs.contains("http://"));
    assert!(
        logs.contains(OWNED_GAMES_PATH),
        "logs name only the endpoint path: {logs}"
    );
    assert!(logs.contains("500"), "logs keep the status: {logs}");
}
