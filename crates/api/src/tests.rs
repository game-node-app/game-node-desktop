use std::time::Duration;

use gamenode_core::callback::{self, Provider};
use gamenode_core::ports::AuthApi;
use gamenode_core::{SessionCredential, SessionError, SignInFailure};
use httpmock::prelude::*;
use serde_json::json;
use url::Url;

use super::*;

fn client(server: &MockServer) -> GameNodeClient {
    GameNodeClient::with_timeout(
        Url::parse(&server.base_url()).unwrap(),
        Duration::from_secs(2),
    )
    .unwrap()
}

fn cred(v: &str) -> SessionCredential {
    SessionCredential::new(v).unwrap()
}

const AUTHORIZATION: &str = "https://accounts.example/o/oauth2/auth?client_id=x&state=STATE123";

fn sign_in_mock<'a>(server: &'a MockServer, set_cookie: Option<&str>) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/auth/sign-in/social")
            .header("capacitor-origin", "app.gamenode://")
            .header("content-type", "application/json")
            .json_body(json!({
                "provider": "discord",
                "callbackURL": callback::success_url("N1"),
                "errorCallbackURL": callback::failure_url("N1"),
            }));
        let then = then
            .status(200)
            .json_body(json!({ "url": AUTHORIZATION, "redirect": false }));
        if let Some(cookie) = set_cookie {
            then.header("set-cookie", cookie);
        }
    })
}

#[tokio::test]
async fn sign_in_sends_native_header_and_callbacks_and_returns_the_proxy_url() {
    let server = MockServer::start_async().await;
    let mock = sign_in_mock(&server, None);
    let url = client(&server)
        .begin_sign_in(
            Provider::Discord,
            &callback::success_url("N1"),
            &callback::failure_url("N1"),
        )
        .await
        .unwrap();
    mock.assert_async().await;

    assert_eq!(url.path(), "/v1/auth/capacitor-authorization-proxy");
    let pairs: Vec<_> = url.query_pairs().into_owned().collect();
    assert_eq!(
        pairs,
        [("authorizationURL".to_string(), AUTHORIZATION.to_string())]
    );
}

#[tokio::test]
async fn oauth_state_cookie_is_transferred_to_the_proxy_decoded() {
    let server = MockServer::start_async().await;
    let _mock = sign_in_mock(
        &server,
        Some("__Secure-better-auth.oauth_state=ab%2Bcd%3D; Max-Age=600; Path=/; HttpOnly"),
    );
    let url = client(&server)
        .begin_sign_in(
            Provider::Discord,
            &callback::success_url("N1"),
            &callback::failure_url("N1"),
        )
        .await
        .unwrap();
    let pairs: Vec<_> = url.query_pairs().into_owned().collect();
    assert!(
        pairs.contains(&("oauthState".to_string(), "ab+cd=".to_string())),
        "{pairs:?}"
    );
    // Re-encoded for the query string, so the server decodes the same value.
    assert!(url.query().unwrap().contains("oauthState=ab%2Bcd%3D"));
}

#[tokio::test]
async fn unrelated_cookies_do_not_become_oauth_state() {
    let server = MockServer::start_async().await;
    let _mock = sign_in_mock(
        &server,
        Some("better-auth.state=signed.value; Path=/; HttpOnly"),
    );
    let url = client(&server)
        .begin_sign_in(
            Provider::Discord,
            &callback::success_url("N1"),
            &callback::failure_url("N1"),
        )
        .await
        .unwrap();
    assert!(!url.query().unwrap().contains("oauthState"));
}

#[tokio::test]
async fn sign_in_failures_are_classified() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let begin = || {
        api.begin_sign_in(
            Provider::Google,
            "app.gamenode://home?desktop_attempt=a",
            "app.gamenode://home/sign-in-failed?desktop_attempt=a",
        )
    };

    for status in [400u16, 403, 500] {
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/auth/sign-in/social");
            then.status(status).json_body(json!({"message": "no"}));
        });
        assert_eq!(
            begin().await.unwrap_err(),
            SignInFailure::ServerUnavailable,
            "{status}"
        );
        mock.delete_async().await;
    }
    // A 200 without a usable authorization URL, including a non-web scheme, is rejected.
    for body in [
        json!({}),
        json!({"url": "javascript:alert(1)"}),
        json!({"url": "file:///etc/passwd"}),
        json!({"url": "nonsense"}),
    ] {
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/auth/sign-in/social");
            then.status(200).json_body(body.clone());
        });
        assert_eq!(
            begin().await.unwrap_err(),
            SignInFailure::ServerUnavailable,
            "{body}"
        );
        mock.delete_async().await;
    }
}

#[tokio::test]
async fn unreachable_server_is_unavailable() {
    let api = GameNodeClient::with_timeout(
        Url::parse("http://127.0.0.1:1").unwrap(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        api.begin_sign_in(Provider::Google, "a", "b")
            .await
            .unwrap_err(),
        SignInFailure::ServerUnavailable
    );
    assert_eq!(
        api.get_session(&cred("t")).await.unwrap_err(),
        SessionError::Unreachable
    );
    assert!(api.sign_out(&cred("t")).await.is_err());
}

#[tokio::test]
async fn get_session_sends_the_signed_bearer_value_and_returns_the_user() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/auth/get-session")
            .header("authorization", "Bearer tok.sig+/==")
            .header("capacitor-origin", "app.gamenode://");
        then.status(200).json_body(json!({
            "session": { "id": "s1", "token": "ignored" },
            "user": { "id": "u1", "name": "Ana", "email": "ana@example.com", "image": null, "role": "user" }
        }));
    });
    let user = client(&server)
        .get_session(&cred("tok.sig+/=="))
        .await
        .unwrap();
    mock.assert_async().await;
    assert_eq!((user.id.as_str(), user.name.as_str()), ("u1", "Ana"));
    assert_eq!(user.email.as_deref(), Some("ana@example.com"));
    assert_eq!(user.image, None);
}

#[tokio::test]
async fn get_session_classifies_rejection_versus_recoverable_failure() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let cases: [(u16, Option<serde_json::Value>, SessionError); 8] = [
        (401, None, SessionError::Rejected),
        (403, None, SessionError::Rejected),
        (200, Some(json!(null)), SessionError::Rejected),
        (
            200,
            Some(json!({"session": null, "user": null})),
            SessionError::Rejected,
        ),
        (500, None, SessionError::Unreachable),
        (502, None, SessionError::Unreachable),
        (429, None, SessionError::Unreachable),
        (200, Some(json!("garbage")), SessionError::Unreachable),
    ];
    for (status, body, expected) in cases {
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/auth/get-session");
            let then = then.status(status);
            if let Some(body) = &body {
                then.json_body(body.clone());
            }
        });
        assert_eq!(
            api.get_session(&cred("t")).await.unwrap_err(),
            expected,
            "{status} {body:?}"
        );
        mock.delete_async().await;
    }
}

#[tokio::test]
async fn slow_servers_time_out_as_unreachable() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/auth/get-session");
        then.status(200)
            .delay(Duration::from_secs(3))
            .json_body(json!(null));
    });
    let api = GameNodeClient::with_timeout(
        Url::parse(&server.base_url()).unwrap(),
        Duration::from_millis(300),
    )
    .unwrap();
    assert_eq!(
        api.get_session(&cred("t")).await.unwrap_err(),
        SessionError::Unreachable
    );
}

#[tokio::test]
async fn sign_out_posts_with_the_bearer_token() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/auth/sign-out")
            .header("authorization", "Bearer tok")
            .json_body(json!({}));
        then.status(200).json_body(json!({"success": true}));
    });
    client(&server).sign_out(&cred("tok")).await.unwrap();
    mock.assert_async().await;

    let failing = MockServer::start_async().await;
    failing.mock(|when, then| {
        when.method(POST).path("/v1/auth/sign-out");
        then.status(500);
    });
    assert!(client(&failing).sign_out(&cred("tok")).await.is_err());
}

#[test]
fn endpoints_respect_a_base_path_and_server_url_validation() {
    let api = GameNodeClient::new(Url::parse("https://example.test/prefix/").unwrap()).unwrap();
    assert_eq!(
        api.endpoint("/get-session").as_str(),
        "https://example.test/prefix/v1/auth/get-session"
    );

    unsafe { std::env::remove_var(SERVER_URL_ENV) };
    assert_eq!(
        server_url_from_env().unwrap().as_str(),
        "https://server.gamenode.app/"
    );
}

#[test]
fn no_diagnostics_contain_the_credential() {
    // Errors are fieldless enums and the client never formats the credential.
    let all = format!(
        "{:?} {:?} {}",
        SessionError::Rejected,
        SignInFailure::ServerUnavailable,
        ClientError::Http
    );
    assert!(!all.contains("tok"));
}
