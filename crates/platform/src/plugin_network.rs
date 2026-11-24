//! HTTPS fetches made on behalf of third-party plugins.
//!
//! A plugin may ask the host to fetch a URL from an origin the user approved. The host is the one
//! on the network, so everything that keeps that safe lives here, in one place:
//!
//! * the URL must be `https`, name a host, carry no user name or password, and its exact origin
//!   (scheme, host and port) must be approved *now*; the caller's `allow` check runs for the
//!   first URL and again for every redirect target, before anything is sent;
//! * the host name is resolved by the client's own resolver, which refuses the connection when
//!   **any** address is loopback, private, link-local, unique-local, shared (CGNAT),
//!   multicast, unspecified, documentation, reserved or an IPv6 form that embeds such an IPv4
//!   address. The client connects to exactly the addresses that passed, so a name cannot pass
//!   the check and then resolve elsewhere; IP-literal hosts get the same test before connecting;
//! * redirects are followed by hand, never by the HTTP library, within [`MAX_REDIRECTS`] hops,
//!   and credentials do not cross origins;
//! * nothing is reused between fetches: no connection pool, no cookies, no proxy (a proxy would
//!   resolve names itself and bypass the address check), no default or GameNode credentials;
//! * the response is bounded by the caller's byte budget (never above [`MAX_RESPONSE_BYTES`]) and
//!   the whole fetch, redirects and body included, by one deadline (never above
//!   [`MAX_TIMEOUT`]).
//!
//! None of this confines a plugin that runs as the user: it is what the *API* does for it.

use std::error::Error as StdError;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{
    AUTHORIZATION, COOKIE, HeaderMap, HeaderName, HeaderValue, LOCATION, PROXY_AUTHORIZATION,
};
use url::{Host, Url};

/// The most bytes a response may hold, whatever the caller asks for.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Package updates have a separate bounded ceiling because ZIP packages are much larger than
/// plugin API responses. This does not widen the ordinary Network.Fetch request budget.
pub const MAX_PACKAGE_RESPONSE_BYTES: usize = 256 * 1024 * 1024;
/// The response budget when the caller names none.
pub const DEFAULT_RESPONSE_BYTES: usize = 1024 * 1024;
/// The longest a whole fetch may take, redirects and body included.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
/// The largest request body.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
/// Redirects followed before giving up.
pub const MAX_REDIRECTS: usize = 5;
/// The longest URL accepted, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Response headers handed back, and their total size.
pub const MAX_RESPONSE_HEADERS: usize = 64;
pub const MAX_RESPONSE_HEADER_BYTES: usize = 32 * 1024;

/// Why a fetch did not produce a response. None of them says where the host is or what it
/// resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// Not https, not an approved origin, a forbidden address, or credentials in the URL.
    #[error("the destination is not allowed")]
    Denied,
    #[error("the response is larger than the budget")]
    TooLarge,
    #[error("the fetch took too long")]
    Timeout,
    #[error("too many redirects")]
    TooManyRedirects,
    /// The host could not be reached or its certificate was not accepted.
    #[error("the destination could not be reached")]
    Unreachable,
    /// A redirect without a usable target.
    #[error("the response is not valid")]
    BadResponse,
}

/// The methods a plugin may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "" | "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            _ => return None,
        })
    }

    fn http(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Head => reqwest::Method::HEAD,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Patch => reqwest::Method::PATCH,
            Self::Delete => reqwest::Method::DELETE,
        }
    }

    fn may_have_body(self) -> bool {
        !matches!(self, Self::Get | Self::Head)
    }
}

/// The most request headers, and the longest value.
pub const MAX_REQUEST_HEADERS: usize = 32;
pub const MAX_REQUEST_HEADER_VALUE_BYTES: usize = 4096;

/// Headers the host sets itself or that would change how the request is framed or routed. A
/// plugin cannot send them.
const FORBIDDEN_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
    "expect",
    "proxy-authorization",
    "proxy-connection",
    "via",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
];

/// Why a request was refused for its shape. Messages are safe to show the plugin.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("url must be an https URL of at most {MAX_URL_BYTES} bytes, without credentials")]
    Url,
    #[error("method must be GET, HEAD, POST, PUT, PATCH or DELETE")]
    Method,
    #[error("{0}")]
    Header(String),
    #[error("{0}")]
    Body(String),
}

/// A request that passed the shape checks. The origin it names is still subject to approval.
#[derive(Clone, Debug)]
pub struct Request {
    url: Url,
    method: Method,
    /// Lowercase names, validated.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    max_response_bytes: usize,
    timeout: Duration,
}

impl Request {
    /// Validates the parts of a plugin's request. `max_response_bytes` 0 means
    /// [`DEFAULT_RESPONSE_BYTES`] and a larger value is capped at [`MAX_RESPONSE_BYTES`];
    /// `timeout_ms` 0 or above [`MAX_TIMEOUT`] means [`MAX_TIMEOUT`].
    pub fn build<'a>(
        url: &str,
        method: &str,
        headers: impl IntoIterator<Item = (&'a str, &'a str)>,
        body: Vec<u8>,
        max_response_bytes: u64,
        timeout_ms: u32,
    ) -> Result<Self, RequestError> {
        let mut url = if url.len() > MAX_URL_BYTES {
            return Err(RequestError::Url);
        } else {
            Url::parse(url).map_err(|_| RequestError::Url)?
        };
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(RequestError::Url);
        }
        url.set_fragment(None);
        if origin_of(&url).is_none() {
            return Err(RequestError::Url);
        }
        let method = Method::parse(method).ok_or(RequestError::Method)?;

        let mut list: Vec<(String, String)> = Vec::new();
        for (name, value) in headers {
            let lowered = name.to_ascii_lowercase();
            if HeaderName::from_bytes(lowered.as_bytes()).is_err() {
                return Err(RequestError::Header(format!(
                    "`{name}` is not a valid header name"
                )));
            }
            if FORBIDDEN_HEADERS.contains(&lowered.as_str()) || lowered.starts_with("proxy-") {
                return Err(RequestError::Header(format!(
                    "the header `{name}` is set by the host and cannot be sent"
                )));
            }
            if value.len() > MAX_REQUEST_HEADER_VALUE_BYTES || HeaderValue::from_str(value).is_err()
            {
                return Err(RequestError::Header(format!(
                    "the value of `{name}` is not valid"
                )));
            }
            if list.iter().any(|(known, _)| *known == lowered) {
                return Err(RequestError::Header(format!(
                    "the header `{name}` is repeated"
                )));
            }
            list.push((lowered, value.to_owned()));
        }
        if list.len() > MAX_REQUEST_HEADERS {
            return Err(RequestError::Header(format!(
                "at most {MAX_REQUEST_HEADERS} headers may be sent"
            )));
        }
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(RequestError::Body(format!(
                "the body is larger than {MAX_REQUEST_BODY_BYTES} bytes"
            )));
        }
        if !body.is_empty() && !method.may_have_body() {
            return Err(RequestError::Body(
                "GET and HEAD requests carry no body".into(),
            ));
        }
        Ok(Self {
            url,
            method,
            headers: list,
            body,
            max_response_bytes: match max_response_bytes {
                0 => DEFAULT_RESPONSE_BYTES,
                bytes => usize::try_from(bytes)
                    .unwrap_or(usize::MAX)
                    .min(MAX_RESPONSE_BYTES),
            },
            timeout: match timeout_ms {
                0 => MAX_TIMEOUT,
                ms => Duration::from_millis(u64::from(ms)).min(MAX_TIMEOUT),
            },
        })
    }

    /// Builds the host-only request used to fetch a signed plugin package archive. It shares the
    /// HTTPS URL checks and fetch client's redirect/DNS/deadline validation while using the
    /// package archive size ceiling instead of the smaller plugin RPC response ceiling.
    pub fn build_package_download(
        url: &str,
        max_response_bytes: u64,
    ) -> Result<Self, RequestError> {
        let mut url = if url.len() > MAX_URL_BYTES {
            return Err(RequestError::Url);
        } else {
            Url::parse(url).map_err(|_| RequestError::Url)?
        };
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || origin_of(&url).is_none()
        {
            return Err(RequestError::Url);
        }
        url.set_fragment(None);
        let budget = usize::try_from(max_response_bytes).unwrap_or(usize::MAX);
        if budget == 0 || budget > MAX_PACKAGE_RESPONSE_BYTES {
            return Err(RequestError::Body(
                "package response exceeds the host limit".into(),
            ));
        }
        Ok(Self {
            url,
            method: Method::Get,
            headers: Vec::new(),
            body: Vec::new(),
            max_response_bytes: budget,
            timeout: MAX_TIMEOUT,
        })
    }

    /// The origin this request is first sent to.
    pub fn origin(&self) -> Option<String> {
        origin_of(&self.url)
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn method(&self) -> &'static str {
        match self.method {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// What came back.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    /// Lowercase names; repeated headers are joined with `, `. Bounded.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The origin the final response came from.
    pub final_origin: String,
}

/// What a client may do. The real one never relaxes either; tests do, to reach a local server.
#[derive(Clone, Copy, Debug)]
struct Policy {
    require_https: bool,
    allow_local: bool,
}

/// An HTTPS client for plugin fetches.
pub struct FetchClient {
    client: reqwest::Client,
    policy: Policy,
}

#[derive(Debug)]
struct Blocked;

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("address not allowed")
    }
}

impl StdError for Blocked {}

/// Resolves with the system resolver and refuses any answer that holds a forbidden address.
struct PublicOnly {
    allow_local: bool,
}

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let allow_local = self.allow_local;
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await
                .map_err(|error| Box::new(error) as Box<dyn StdError + Send + Sync>)?
                .collect();
            // One forbidden address spoils the answer: a name that also points inside is not
            // one to talk to.
            if found.is_empty() || (!allow_local && found.iter().any(|a| !is_public(a.ip()))) {
                return Err(Box::new(Blocked) as Box<dyn StdError + Send + Sync>);
            }
            Ok(Box::new(found.into_iter()) as Addrs)
        })
    }
}

/// Whether `ip` is an address a plugin may be sent to: a public, routable one.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0 // "this network"
        || (a == 100 && (64..=127).contains(&b)) // shared address space (CGNAT)
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 240) // reserved
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // IPv4 carried inside IPv6 is judged as the IPv4 address it is.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let embedded = |high: u16, low: u16| {
        Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8)
    };
    // 64:ff9b::/96 (NAT64) and 2002::/16 (6to4) embed an IPv4 address.
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return public_v4(embedded(segments[6], segments[7]));
    }
    if segments[0] == 0x2002 {
        return public_v4(embedded(segments[1], segments[2]));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (segments[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (segments[0] & 0xffc0) == 0xfec0 // deprecated site-local
        || segments[..6] == [0, 0, 0, 0, 0, 0] // IPv4-compatible (deprecated)
        || (segments[0] == 0x2001 && segments[1] == 0) // Teredo
        || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
        || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0])) // discard-only
}

/// The exact origin of `url` as text (`https://host` or `https://host:port`), or `None` when it
/// is not an https URL with a host.
pub fn origin_of(url: &Url) -> Option<String> {
    // A name is letters, digits, hyphens and dots (after punycode); `*` and the like are not part
    // of a host, so no origin can stand for several.
    let named = match url.host() {
        Some(Host::Domain(name)) => name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')),
        Some(_) => true,
        None => false,
    };
    (url.scheme() == "https" && named).then(|| url.origin().ascii_serialization())
}

/// Why a redirect or request target is refused, before any connection.
fn check_target(url: &Url, policy: Policy) -> Result<String, FetchError> {
    if url.as_str().len() > MAX_URL_BYTES
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host().is_none()
    {
        return Err(FetchError::Denied);
    }
    let secure = url.scheme() == "https";
    if !secure && (policy.require_https || url.scheme() != "http") {
        return Err(FetchError::Denied);
    }
    // An address in the URL never reaches the resolver, so it is judged here.
    let literal = match url.host() {
        Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        _ => None,
    };
    if literal.is_some_and(|ip| !policy.allow_local && !is_public(ip)) {
        return Err(FetchError::Denied);
    }
    Ok(url.origin().ascii_serialization())
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

/// Finds the cause of a failed send: a refused address, a timeout, or anything else.
fn classify(error: &reqwest::Error) -> FetchError {
    let mut source: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(current) = source {
        if current.is::<Blocked>() {
            return FetchError::Denied;
        }
        source = current.source();
    }
    if error.is_timeout() {
        FetchError::Timeout
    } else if error.is_builder() {
        // The client refuses a scheme it was told not to use.
        FetchError::Denied
    } else {
        FetchError::Unreachable
    }
}

impl FetchClient {
    /// The client plugins use: https only, public addresses only.
    pub fn new() -> Result<Self, reqwest::Error> {
        Self::build(Policy {
            require_https: true,
            allow_local: false,
        })
    }

    fn build(policy: Policy) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .https_only(policy.require_https)
            .dns_resolver(Arc::new(PublicOnly {
                allow_local: policy.allow_local,
            }))
            .user_agent(concat!(
                "GameNode-Desktop/",
                env!("CARGO_PKG_VERSION"),
                " plugin-fetch"
            ))
            .build()?;
        Ok(Self { client, policy })
    }

    /// A client that may talk plain HTTP to local servers, for tests only.
    #[cfg(test)]
    fn for_tests(allow_local: bool) -> Self {
        Self::build(Policy {
            require_https: false,
            allow_local,
        })
        .unwrap()
    }

    /// Fetches `request`. `allow` is asked, with the exact origin, before the first request and
    /// before every redirect is followed; returning false ends the fetch with
    /// [`FetchError::Denied`] and nothing is sent to that origin.
    pub async fn fetch(
        &self,
        request: Request,
        allow: impl Fn(&str) -> bool + Send + Sync,
    ) -> Result<Response, FetchError> {
        self.fetch_with_origin_check(request, |origin| std::future::ready(allow(&origin)))
            .await
    }

    /// Like `fetch`, with an asynchronous authorization check before each request.
    pub async fn fetch_with_origin_check<F>(
        &self,
        request: Request,
        allow: impl Fn(String) -> F + Send + Sync,
    ) -> Result<Response, FetchError>
    where
        F: std::future::Future<Output = bool> + Send,
    {
        let deadline = request.timeout;
        tokio::time::timeout(deadline, self.run(request, allow))
            .await
            .map_err(|_| FetchError::Timeout)?
    }

    async fn run<F>(
        &self,
        request: Request,
        allow: impl Fn(String) -> F + Send + Sync,
    ) -> Result<Response, FetchError>
    where
        F: std::future::Future<Output = bool> + Send,
    {
        let budget = request.max_response_bytes;
        let Request {
            mut url,
            mut method,
            headers,
            mut body,
            ..
        } = request;
        let mut headers: HeaderMap = headers
            .iter()
            .map(|(name, value)| {
                Some((
                    HeaderName::from_bytes(name.as_bytes()).ok()?,
                    HeaderValue::from_str(value).ok()?,
                ))
            })
            .collect::<Option<_>>()
            .ok_or(FetchError::Denied)?;
        for hop in 0..=MAX_REDIRECTS {
            let origin = check_target(&url, self.policy)?;
            if !allow(origin.clone()).await {
                return Err(FetchError::Denied);
            }
            let mut sending = self
                .client
                .request(method.http(), url.clone())
                .headers(headers.clone());
            if !body.is_empty() {
                sending = sending.body(body.clone());
            }
            let mut response = sending.send().await.map_err(|error| classify(&error))?;
            let status = response.status();

            let target = if is_redirect(status) {
                response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .map(|location| url.join(location).map_err(|_| FetchError::BadResponse))
            } else {
                None
            };
            if let Some(target) = target {
                let next = target?;
                if hop == MAX_REDIRECTS {
                    return Err(FetchError::TooManyRedirects);
                }
                // A redirect response's body is never read.
                drop(response);
                // Credentials stay with the origin they were meant for.
                if next.origin() != url.origin() {
                    headers.remove(AUTHORIZATION);
                    headers.remove(COOKIE);
                    headers.remove(PROXY_AUTHORIZATION);
                }
                // As browsers do: 303, and POST after 301/302, continue as a body-less GET.
                let switch = status.as_u16() == 303 && method != Method::Head
                    || matches!(status.as_u16(), 301 | 302) && method == Method::Post;
                if switch {
                    method = Method::Get;
                    body = Vec::new();
                    headers.remove(reqwest::header::CONTENT_TYPE);
                }
                url = next;
                continue;
            }

            if response
                .content_length()
                .is_some_and(|length| length > budget as u64)
            {
                return Err(FetchError::TooLarge);
            }
            let mut collected = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| classify(&error))? {
                if collected.len() + chunk.len() > budget {
                    return Err(FetchError::TooLarge);
                }
                collected.extend_from_slice(&chunk);
            }
            return Ok(Response {
                status: status.as_u16(),
                headers: header_list(response.headers()),
                body: collected,
                final_origin: origin,
            });
        }
        Err(FetchError::TooManyRedirects)
    }
}

/// The response headers a plugin gets: lowercase names, repeated ones joined, bounded in count
/// and size (the rest are dropped).
fn header_list(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = Vec::new();
    let mut bytes = 0;
    for (name, value) in headers {
        let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
        if let Some((_, joined)) = list.iter_mut().find(|(known, _)| known == name.as_str()) {
            if bytes + value.len() + 2 > MAX_RESPONSE_HEADER_BYTES {
                continue;
            }
            bytes += value.len() + 2;
            joined.push_str(", ");
            joined.push_str(&value);
            continue;
        }
        if list.len() >= MAX_RESPONSE_HEADERS
            || bytes + name.as_str().len() + value.len() > MAX_RESPONSE_HEADER_BYTES
        {
            continue;
        }
        bytes += name.as_str().len() + value.len();
        list.push((name.as_str().to_owned(), value));
    }
    list
}

#[cfg(test)]
mod tests;
