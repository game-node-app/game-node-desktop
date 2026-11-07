//! The `app.gamenode://` callback contract.
//!
//! Mirrors the server's mobile browser sign-in (`better-auth-capacitor`): after the provider
//! flow the server redirects to the `callbackURL` we supplied and appends a `cookie` query
//! parameter that holds the `Set-Cookie` header(s). We add one `desktop_attempt` correlation
//! value to our callback URLs and parse the redirect strictly, as data, never logging it.

use std::fmt;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::credential::SessionCredential;

/// The native origin the server trusts (`capacitor-origin` header and callback scheme).
pub const NATIVE_ORIGIN: &str = "app.gamenode://";
/// URL scheme of GameNode callbacks.
pub const SCHEME: &str = "app.gamenode";
/// Callback host for both outcomes.
pub const HOST: &str = "home";
/// Extra path that distinguishes the failure callback from the success callback.
pub const FAILURE_PATH: &str = "/sign-in-failed";
/// Query key that carries our single-use correlation value.
pub const ATTEMPT_PARAM: &str = "desktop_attempt";
/// Query key the server appends with the `Set-Cookie` header value(s).
pub const COOKIE_PARAM: &str = "cookie";
/// Base name of the Better Auth session cookie.
pub const SESSION_COOKIE: &str = "better-auth.session_token";
/// Prefix Better Auth puts on cookies when the server base URL is HTTPS.
pub const SECURE_PREFIX: &str = "__Secure-";
/// Longest accepted callback URL; larger input is rejected before parsing.
pub const MAX_CALLBACK_LEN: usize = 8 * 1024;
/// Longest accepted correlation value and session credential.
const MAX_NONCE_LEN: usize = 128;
const MAX_CREDENTIAL_LEN: usize = 4 * 1024;

use serde::{Deserialize, Serialize};

/// Identity providers that GameNode's existing sign-in supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Google,
    Discord,
    Twitter,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Google, Provider::Discord, Provider::Twitter];

    /// The identifier the server expects in `POST /v1/auth/sign-in/social`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Discord => "discord",
            Self::Twitter => "twitter",
        }
    }
}

/// The callback URLs for one attempt.
pub fn success_url(nonce: &str) -> String {
    format!("{SCHEME}://{HOST}?{ATTEMPT_PARAM}={nonce}")
}

pub fn failure_url(nonce: &str) -> String {
    format!("{SCHEME}://{HOST}{FAILURE_PATH}?{ATTEMPT_PARAM}={nonce}")
}

/// Why a callback was rejected. Never contains the URL or the credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackError {
    TooLong,
    Malformed,
    WrongTarget,
    DuplicateField,
    MissingCorrelation,
    MissingCredential,
    InvalidCredential,
    /// Syntactically valid, but no sign-in is active or the attempt can no longer accept it.
    NoActiveAttempt,
    Expired,
    CorrelationMismatch,
}

impl fmt::Display for CallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLong => "callback too long",
            Self::Malformed => "callback malformed",
            Self::WrongTarget => "callback target unexpected",
            Self::DuplicateField => "callback has a duplicated field",
            Self::MissingCorrelation => "callback lacks a correlation value",
            Self::MissingCredential => "callback lacks a session credential",
            Self::InvalidCredential => "callback credential invalid",
            Self::NoActiveAttempt => "no sign-in attempt is active",
            Self::Expired => "sign-in attempt expired",
            Self::CorrelationMismatch => "callback does not match the sign-in attempt",
        })
    }
}

impl std::error::Error for CallbackError {}

/// A syntactically valid callback. It still has to match the active attempt.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCallback {
    pub nonce: String,
    pub outcome: CallbackOutcome,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    Success(SessionCredential),
    Failure,
}

/// Parses a raw callback URL. Pure: no state, no logging.
pub fn parse(raw: &str) -> Result<ParsedCallback, CallbackError> {
    if raw.len() > MAX_CALLBACK_LEN {
        return Err(CallbackError::TooLong);
    }
    let url = Url::parse(raw).map_err(|_| CallbackError::Malformed)?;
    if url.scheme() != SCHEME
        || url.host_str() != Some(HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(CallbackError::WrongTarget);
    }
    let is_failure = match url.path() {
        "" | "/" => false,
        FAILURE_PATH => true,
        _ => return Err(CallbackError::WrongTarget),
    };

    let mut nonce = None;
    let mut cookie = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            ATTEMPT_PARAM => &mut nonce,
            COOKIE_PARAM => &mut cookie,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err(CallbackError::DuplicateField);
        }
    }

    let nonce = nonce
        .filter(|n| valid_nonce(n))
        .ok_or(CallbackError::MissingCorrelation)?;
    if is_failure {
        return Ok(ParsedCallback {
            nonce,
            outcome: CallbackOutcome::Failure,
        });
    }
    let cookie = cookie.ok_or(CallbackError::MissingCredential)?;
    let credential = session_credential(&cookie)?;
    Ok(ParsedCallback {
        nonce,
        outcome: CallbackOutcome::Success(credential),
    })
}

fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= MAX_NONCE_LEN
        && nonce
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Extracts the signed session value from the `Set-Cookie` text in the `cookie` parameter.
///
/// Accepts `better-auth.session_token` and its `__Secure-` form, requires exactly one of them,
/// percent-decodes once and keeps the signed value unchanged.
pub fn session_credential(cookie_param: &str) -> Result<SessionCredential, CallbackError> {
    let mut found: Option<&str> = None;
    // Several cookies are joined with ", " and attributes with "; "; encoded values contain neither.
    for segment in cookie_param.split([';', ',']) {
        let Some((name, value)) = segment.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        let name = name.strip_prefix(SECURE_PREFIX).unwrap_or(name);
        if name == SESSION_COOKIE && found.replace(value.trim()).is_some() {
            return Err(CallbackError::DuplicateField);
        }
    }
    let raw = found.ok_or(CallbackError::MissingCredential)?;
    let decoded = percent_decode_str(raw)
        .decode_utf8()
        .map_err(|_| CallbackError::InvalidCredential)?;
    if decoded.is_empty()
        || decoded.len() > MAX_CREDENTIAL_LEN
        || decoded.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(CallbackError::InvalidCredential);
    }
    SessionCredential::new(decoded.into_owned()).ok_or(CallbackError::InvalidCredential)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Redacted fixture shaped like the server's redirect (see game-node-server
    // auth.integration.spec.ts): `cookie` carries the encoded Set-Cookie header.
    const SIGNED: &str = "tok123.c2lnbmF0dXJl%2BfA%3D%3D";

    fn callback(path_and_query: &str) -> String {
        format!("app.gamenode://home{path_and_query}")
    }

    fn cookie_param(name: &str) -> String {
        let header = format!("{name}={SIGNED}; Max-Age=604800; Path=/; HttpOnly; SameSite=Lax");
        url::form_urlencoded::byte_serialize(header.as_bytes()).collect()
    }

    fn success(name: &str) -> String {
        callback(&format!(
            "?desktop_attempt=n-1_A&cookie={}",
            cookie_param(name)
        ))
    }

    #[test]
    fn builds_exact_callback_urls() {
        assert_eq!(
            success_url("abc"),
            "app.gamenode://home?desktop_attempt=abc"
        );
        assert_eq!(
            failure_url("abc"),
            "app.gamenode://home/sign-in-failed?desktop_attempt=abc"
        );
    }

    #[test]
    fn parses_plain_and_secure_prefixed_cookies() {
        for name in [
            "better-auth.session_token",
            "__Secure-better-auth.session_token",
        ] {
            let parsed = parse(&success(name)).unwrap();
            assert_eq!(parsed.nonce, "n-1_A");
            let CallbackOutcome::Success(credential) = parsed.outcome else {
                panic!()
            };
            // Decoded once; the signature keeps its '+' and '=' characters.
            assert_eq!(credential.expose(), "tok123.c2lnbmF0dXJl+fA==");
        }
    }

    #[test]
    fn parses_session_token_among_other_cookies() {
        let header = format!(
            "better-auth.session_data=zzz; Path=/, __Secure-better-auth.session_token={SIGNED}; Max-Age=1"
        );
        let encoded: String = url::form_urlencoded::byte_serialize(header.as_bytes()).collect();
        let raw = callback(&format!("?desktop_attempt=n&cookie={encoded}"));
        let CallbackOutcome::Success(c) = parse(&raw).unwrap().outcome else {
            panic!()
        };
        assert_eq!(c.expose(), "tok123.c2lnbmF0dXJl+fA==");
    }

    #[test]
    fn parses_the_failure_callback() {
        let parsed =
            parse("app.gamenode://home/sign-in-failed?desktop_attempt=n9&error=denied").unwrap();
        assert_eq!(
            parsed,
            ParsedCallback {
                nonce: "n9".into(),
                outcome: CallbackOutcome::Failure
            }
        );
    }

    #[test]
    fn rejects_wrong_scheme_host_path_and_extras() {
        let c = cookie_param("better-auth.session_token");
        for raw in [
            format!("https://home?desktop_attempt=n&cookie={c}"),
            format!("app.gamenode://evil?desktop_attempt=n&cookie={c}"),
            format!("app.gamenode://home/other?desktop_attempt=n&cookie={c}"),
            format!("app.gamenode://home:80?desktop_attempt=n&cookie={c}"),
            format!("app.gamenode://user@home?desktop_attempt=n&cookie={c}"),
            format!("app.gamenode://home?desktop_attempt=n&cookie={c}#frag"),
            "app.gamenode://home/sign-in-failed/extra?desktop_attempt=n".to_owned(),
        ] {
            assert_eq!(
                parse(&raw).unwrap_err(),
                CallbackError::WrongTarget,
                "{raw}"
            );
        }
        assert_eq!(parse("not a url").unwrap_err(), CallbackError::Malformed);
        assert_eq!(parse("").unwrap_err(), CallbackError::Malformed);
    }

    #[test]
    fn rejects_duplicate_missing_and_invalid_fields() {
        let c = cookie_param("better-auth.session_token");
        let cases = [
            (
                format!("?desktop_attempt=a&desktop_attempt=b&cookie={c}"),
                CallbackError::DuplicateField,
            ),
            (
                format!("?desktop_attempt=a&cookie={c}&cookie={c}"),
                CallbackError::DuplicateField,
            ),
            (format!("?cookie={c}"), CallbackError::MissingCorrelation),
            (
                "?desktop_attempt=&cookie=x".to_string(),
                CallbackError::MissingCorrelation,
            ),
            (
                format!("?desktop_attempt=a b&cookie={c}"),
                CallbackError::MissingCorrelation,
            ),
            (
                "?desktop_attempt=a".to_string(),
                CallbackError::MissingCredential,
            ),
            (
                "?desktop_attempt=a&cookie=other%3Dvalue".to_string(),
                CallbackError::MissingCredential,
            ),
            (
                "?desktop_attempt=a&cookie=better-auth.session_token%3D".to_string(),
                CallbackError::InvalidCredential,
            ),
            (
                "?desktop_attempt=a&cookie=better-auth.session_token%3D%25FF".to_string(),
                CallbackError::InvalidCredential,
            ),
        ];
        for (query, expected) in cases {
            assert_eq!(parse(&callback(&query)).unwrap_err(), expected, "{query}");
        }
        // Two token cookies (plain and secure) are ambiguous.
        let two = "better-auth.session_token=a, __Secure-better-auth.session_token=b";
        let encoded: String = url::form_urlencoded::byte_serialize(two.as_bytes()).collect();
        assert_eq!(
            parse(&callback(&format!("?desktop_attempt=a&cookie={encoded}"))).unwrap_err(),
            CallbackError::DuplicateField
        );
    }

    #[test]
    fn rejects_oversized_input_before_parsing() {
        let raw = format!(
            "app.gamenode://home?desktop_attempt={}",
            "a".repeat(MAX_CALLBACK_LEN)
        );
        assert_eq!(parse(&raw).unwrap_err(), CallbackError::TooLong);
    }

    #[test]
    fn errors_and_debug_output_exclude_the_credential() {
        let raw = success("better-auth.session_token");
        let parsed = parse(&raw).unwrap();
        assert!(!format!("{parsed:?}").contains("tok123"));
        for err in [
            CallbackError::Malformed,
            CallbackError::InvalidCredential,
            CallbackError::DuplicateField,
        ] {
            assert!(!format!("{err} {err:?}").contains("tok123"));
        }
    }
}
