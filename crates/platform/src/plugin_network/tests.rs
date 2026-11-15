use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::header::{HeaderName, HeaderValue};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

enum Reply {
    Raw(Vec<u8>),
    /// Never answers.
    Hang,
}

struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}{path}", self.port)).unwrap()
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn last_request(&self) -> String {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

/// A local HTTP server: every connection is one request, answered by `handler`.
async fn serve(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let (counted, recorded) = (hits.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let (handler, recorded) = (handler.clone(), recorded.clone());
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buffer[..head_end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while buffer.len() < head_end + length {
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                }
                let raw = String::from_utf8_lossy(&buffer).into_owned();
                recorded.lock().unwrap().push(raw.clone());
                match handler(&raw) {
                    Reply::Raw(bytes) => {
                        let _ = stream.write_all(&bytes).await;
                        let _ = stream.shutdown().await;
                    }
                    Reply::Hang => tokio::time::sleep(Duration::from_secs(120)).await,
                }
            });
        }
    });
    Server { port, hits, seen }
}

fn ok(body: &str) -> Reply {
    Reply::Raw(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nX-Reply: yes\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
    )
}

fn redirect(code: u16, location: &str) -> Reply {
    Reply::Raw(
        format!(
            "HTTP/1.1 {code} Redirect\r\nLocation: {location}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nsecret"
        )
        .into_bytes(),
    )
}

/// A GET of `url`. The tests talk to plain-HTTP local servers, so the request is assembled
/// directly rather than through [`Request::build`], which only takes https.
fn get(url: Url) -> Request {
    Request {
        url,
        method: Method::Get,
        headers: Vec::new(),
        body: Vec::new(),
        max_response_bytes: DEFAULT_RESPONSE_BYTES,
        timeout: Duration::from_secs(10),
    }
}

fn with_header(mut request: Request, name: &str, value: &str) -> Request {
    request.headers.push((name.to_owned(), value.to_owned()));
    request
}

fn local() -> FetchClient {
    FetchClient::for_tests(true)
}

fn any(_: &str) -> bool {
    true
}

// ---- addresses and URLs ----

#[test]
fn only_public_addresses_are_allowed() {
    let public = [
        "8.8.8.8",
        "1.1.1.1",
        "93.184.216.34",
        "172.32.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "2606:4700:4700::1111",
        "2a00:1450:4001::200e",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "2002:808:808::1",
    ];
    for text in public {
        assert!(is_public(text.parse().unwrap()), "{text} should be public");
    }
    let forbidden = [
        "127.0.0.1",
        "127.255.255.254",
        "0.0.0.0",
        "0.1.2.3",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "100.127.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.255",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "fe80::1",
        "fec0::1",
        "fc00::1",
        "fd12:3456::1",
        "ff02::1",
        "2001:db8::1",
        "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::ffff:169.254.169.254",
        "64:ff9b::7f00:1",
        "64:ff9b::a00:1",
        "2002:7f00:1::1",
        "2002:c0a8:101::1",
        "::1.2.3.4",
        "100::1",
    ];
    for text in forbidden {
        assert!(
            !is_public(text.parse().unwrap()),
            "{text} should be refused"
        );
    }
}

#[test]
fn an_origin_is_scheme_host_and_port_exactly() {
    let origin = |text: &str| origin_of(&Url::parse(text).unwrap());
    assert_eq!(
        origin("https://Example.COM/a/b?q=1#f").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        origin("https://example.com:443/").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        origin("https://example.com:8443/").as_deref(),
        Some("https://example.com:8443")
    );
    assert_eq!(
        origin("https://[2606:4700::1]:8443/").as_deref(),
        Some("https://[2606:4700::1]:8443")
    );
    assert_eq!(
        origin("https://bücher.example/").as_deref(),
        Some("https://xn--bcher-kva.example")
    );
    assert_eq!(origin("http://example.com/"), None);
    assert_eq!(origin("ftp://example.com/"), None);
    // Different hosts, subdomains and ports are different origins.
    assert_ne!(
        origin("https://example.com/"),
        origin("https://www.example.com/")
    );
    assert_ne!(
        origin("https://example.com/"),
        origin("https://example.com:444/")
    );
    // A host that merely starts like an approved one is another origin.
    assert_ne!(
        origin("https://example.com.evil.test/"),
        origin("https://example.com/")
    );
}

#[test]
fn targets_must_be_https_public_and_free_of_credentials() {
    let strict = Policy {
        require_https: true,
        allow_local: false,
    };
    let check = |text: &str| check_target(&Url::parse(text).unwrap(), strict);
    assert_eq!(
        check("https://example.com/path?x=1").as_deref(),
        Ok("https://example.com")
    );
    assert_eq!(check("https://8.8.8.8/").as_deref(), Ok("https://8.8.8.8"));
    for denied in [
        "http://example.com/",
        "ftp://example.com/",
        "file:///etc/passwd",
        "https://user@example.com/",
        "https://user:pass@example.com/",
        "https://127.0.0.1/",
        "https://127.1/",
        "https://2130706433/",
        "https://0x7f.0.0.1/",
        "https://[::1]/",
        "https://[::ffff:127.0.0.1]/",
        "https://10.1.2.3:8443/",
        "https://192.168.0.10/",
        "https://169.254.169.254/latest/meta-data/",
        "https://[fd00::1]/",
        "https://0.0.0.0/",
    ] {
        assert_eq!(check(denied), Err(FetchError::Denied), "{denied}");
    }
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
    assert_eq!(check(&long), Err(FetchError::Denied));
}

// ---- the happy path ----

#[tokio::test]
async fn a_fetch_returns_status_headers_body_and_the_final_origin() {
    let server = serve(|_| ok("hello")).await;
    let request = with_header(get(server.url("/page?x=1")), "x-custom", "mine");
    let response = local().fetch(request, any).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello");
    assert_eq!(response.final_origin, server.origin());
    let headers: std::collections::HashMap<_, _> = response.headers.into_iter().collect();
    assert_eq!(headers["x-reply"], "yes");
    assert_eq!(headers["content-type"], "text/plain");

    let seen = server.last_request().to_lowercase();
    assert!(seen.starts_with("get /page?x=1 http/1.1"), "{seen}");
    assert!(seen.contains("x-custom: mine"));
    // Nothing of GameNode's own: no cookie, no authorization, no referer.
    assert!(seen.contains("user-agent: gamenode-desktop/"), "{seen}");
    for forbidden in [
        "cookie:",
        "authorization:",
        "referer:",
        "proxy-authorization:",
    ] {
        assert!(!seen.contains(forbidden), "{forbidden} in {seen}");
    }
}

#[tokio::test]
async fn a_body_is_sent_with_its_method_and_error_statuses_are_responses() {
    let server = serve(|raw| {
        if raw.starts_with("POST") && raw.ends_with("payload") {
            Reply::Raw(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 3\r\nConnection: close\r\n\r\nnah"
                    .to_vec(),
            )
        } else {
            ok("wrong")
        }
    })
    .await;
    let mut request = get(server.url("/"));
    request.method = Method::Post;
    request.body = b"payload".to_vec();
    let response = local().fetch(request, any).await.unwrap();
    assert_eq!(
        (response.status, response.body.as_slice()),
        (404, &b"nah"[..])
    );
}

#[tokio::test]
async fn head_requests_return_no_body() {
    let server = serve(|_| {
        Reply::Raw(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n".to_vec())
    })
    .await;
    let mut request = get(server.url("/"));
    request.method = Method::Head;
    let response = local().fetch(request, any).await.unwrap();
    assert!(response.body.is_empty());
}

// ---- bounds ----

#[tokio::test]
async fn a_response_over_the_budget_is_refused_not_cut_short() {
    let big = "x".repeat(1000);
    let server = serve(move |_| ok(&big)).await;
    let mut request = get(server.url("/"));
    request.max_response_bytes = 999;
    assert_eq!(
        local().fetch(request.clone(), any).await.unwrap_err(),
        FetchError::TooLarge
    );
    request.max_response_bytes = 1000;
    assert_eq!(local().fetch(request, any).await.unwrap().body.len(), 1000);
}

#[tokio::test]
async fn a_body_that_outgrows_the_budget_while_streaming_is_refused() {
    // No Content-Length: the size is only known by reading.
    let server = serve(|_| {
        let mut bytes = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'y', 5000));
        Reply::Raw(bytes)
    })
    .await;
    let mut request = get(server.url("/"));
    request.max_response_bytes = 4000;
    assert_eq!(
        local().fetch(request, any).await.unwrap_err(),
        FetchError::TooLarge
    );
}

#[tokio::test]
async fn the_budget_never_exceeds_the_hard_cap() {
    let server = serve(|_| ok("small")).await;
    let mut request = get(server.url("/"));
    request.max_response_bytes = usize::MAX;
    assert_eq!(local().fetch(request, any).await.unwrap().body, b"small");
    assert_eq!(MAX_RESPONSE_BYTES, 8 * 1024 * 1024);
}

#[tokio::test]
async fn one_deadline_covers_the_whole_fetch() {
    let hanging = serve(|_| Reply::Hang).await;
    let mut request = get(hanging.url("/"));
    request.timeout = Duration::from_millis(150);
    let started = std::time::Instant::now();
    assert_eq!(
        local().fetch(request, any).await.unwrap_err(),
        FetchError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(5));

    // A body that never finishes counts too.
    let slow = serve(|_| {
        Reply::Raw(
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nstart".to_vec(),
        )
    })
    .await;
    let mut request = get(slow.url("/"));
    request.timeout = Duration::from_millis(300);
    let error = local().fetch(request, any).await.unwrap_err();
    assert!(
        matches!(error, FetchError::Timeout | FetchError::Unreachable),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_deadline_is_capped_at_the_maximum() {
    let server = serve(|_| ok("fast")).await;
    let mut request = get(server.url("/"));
    request.timeout = Duration::from_secs(3600);
    assert_eq!(local().fetch(request, any).await.unwrap().body, b"fast");
    assert_eq!(MAX_TIMEOUT, Duration::from_secs(30));
}

// ---- redirects ----

#[tokio::test]
async fn redirects_are_followed_within_the_approved_origin() {
    let server = serve(|raw| {
        if raw.starts_with("GET /start") {
            redirect(302, "/middle")
        } else if raw.starts_with("GET /middle") {
            redirect(301, "http://127.0.0.1:1/ignored-port-replaced")
        } else {
            ok("end")
        }
    })
    .await;
    // The second hop names a different port: it is a different origin and must be asked about.
    let asked = Mutex::new(Vec::new());
    let origin = server.origin();
    let error = local()
        .fetch(get(server.url("/start")), |candidate| {
            asked.lock().unwrap().push(candidate.to_owned());
            candidate == origin
        })
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::Denied);
    assert_eq!(
        asked.lock().unwrap().as_slice(),
        [
            origin.clone(),
            origin.clone(),
            "http://127.0.0.1:1".to_owned()
        ]
    );
    assert_eq!(
        server.hits(),
        2,
        "the unapproved origin was never contacted"
    );

    let server = serve(|raw| {
        if raw.starts_with("GET /start") {
            redirect(302, "/end")
        } else {
            ok("end")
        }
    })
    .await;
    let origin = server.origin();
    let response = local()
        .fetch(get(server.url("/start")), |c| c == origin)
        .await
        .unwrap();
    assert_eq!(
        (response.body.as_slice(), response.final_origin),
        (&b"end"[..], origin)
    );
}

#[tokio::test]
async fn a_redirect_to_an_unapproved_origin_is_refused_before_anything_is_sent_there() {
    let other = serve(|_| ok("loot")).await;
    let target = other.url("/loot").to_string();
    let first = serve(move |_| redirect(302, &target)).await;
    let approved = first.origin();
    let error = local()
        .fetch(get(first.url("/")), |origin| origin == approved)
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::Denied);
    assert_eq!(
        other.hits(),
        0,
        "no connection reached the unapproved origin"
    );
}

#[tokio::test]
async fn update_package_redirect_checks_the_new_origin_before_download() {
    let destination = serve(|_| ok("signed package archive")).await;
    let target = destination.url("/release.zip").to_string();
    let feed = serve(move |_| redirect(302, &target)).await;
    let only_feed = feed.origin();
    let error = local()
        .fetch(get(feed.url("/plugin.zip")), |origin| origin == only_feed)
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::Denied);
    assert_eq!(destination.hits(), 0);

    let allowed = [feed.origin(), destination.origin()];
    let response = local()
        .fetch(get(feed.url("/plugin.zip")), |origin| {
            allowed.contains(&origin.to_owned())
        })
        .await
        .unwrap();
    assert_eq!(response.body, b"signed package archive");
}

#[tokio::test]
async fn a_widened_approval_is_checked_at_every_hop() {
    let other = serve(|_| ok("fine")).await;
    let target = other.url("/").to_string();
    let first = serve(move |_| redirect(302, &target)).await;
    let both = [first.origin(), other.origin()];
    let response = local()
        .fetch(get(first.url("/")), |o| both.contains(&o.to_owned()))
        .await
        .unwrap();
    assert_eq!(response.body, b"fine");
    // Revoking one origin between hops stops the second.
    let only_first = [first.origin()];
    let error = local()
        .fetch(get(first.url("/")), |o| only_first.contains(&o.to_owned()))
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::Denied);
}

#[tokio::test]
async fn redirect_chains_are_bounded() {
    let server = serve(|_| redirect(302, "/again")).await;
    let origin = server.origin();
    let error = local()
        .fetch(get(server.url("/")), |o| o == origin)
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::TooManyRedirects);
    assert_eq!(server.hits(), MAX_REDIRECTS + 1);
}

#[tokio::test]
async fn credentials_do_not_follow_a_redirect_to_another_origin() {
    let other = serve(|_| ok("done")).await;
    let target = other.url("/").to_string();
    let first = serve(move |raw| {
        if raw.starts_with("GET /same") {
            redirect(302, "/landing")
        } else if raw.starts_with("GET /landing") {
            ok("landed")
        } else {
            redirect(302, &target)
        }
    })
    .await;
    let origins = [first.origin(), other.origin()];
    let with_credentials = |url| {
        let request = with_header(get(url), "authorization", "Bearer secret");
        let request = with_header(request, "cookie", "sid=1");
        with_header(request, "x-keep", "yes")
    };

    // Same origin: the plugin's own credentials stay.
    local()
        .fetch(with_credentials(first.url("/same")), |o| {
            origins.contains(&o.to_owned())
        })
        .await
        .unwrap();
    let landing = first.last_request().to_lowercase();
    assert!(
        landing.contains("authorization: bearer secret") && landing.contains("cookie: sid=1"),
        "{landing}"
    );

    // Another origin: they are dropped, other headers stay.
    local()
        .fetch(with_credentials(first.url("/cross")), |o| {
            origins.contains(&o.to_owned())
        })
        .await
        .unwrap();
    let crossed = other.last_request().to_lowercase();
    assert!(
        !crossed.contains("authorization") && !crossed.contains("cookie"),
        "{crossed}"
    );
    assert!(crossed.contains("x-keep: yes"));
}

#[tokio::test]
async fn redirect_methods_follow_the_usual_rules() {
    let server = serve(|raw| {
        let path = raw.split_whitespace().nth(1).unwrap_or("").to_owned();
        match path.as_str() {
            "/303" => redirect(303, "/final"),
            "/307" => redirect(307, "/final"),
            "/302" => redirect(302, "/final"),
            _ => ok("ok"),
        }
    })
    .await;
    let origin = server.origin();
    let post = |path: &str| {
        let mut request = with_header(get(server.url(path)), "content-type", "text/plain");
        request.method = Method::Post;
        request.body = b"data".to_vec();
        request
    };
    for (path, method, keeps_body) in [
        ("/303", "GET", false),
        ("/302", "GET", false),
        ("/307", "POST", true),
    ] {
        local().fetch(post(path), |o| o == origin).await.unwrap();
        let landed = server.last_request();
        assert!(
            landed.starts_with(&format!("{method} /final")),
            "{path}: {landed}"
        );
        assert_eq!(landed.ends_with("data"), keeps_body, "{path}: {landed}");
    }
}

#[tokio::test]
async fn bad_redirects_are_errors_and_a_redirect_without_a_target_is_a_response() {
    let bad = serve(|_| redirect(302, "http://[::bad")).await;
    let origin = bad.origin();
    assert_eq!(
        local()
            .fetch(get(bad.url("/")), |o| o == origin)
            .await
            .unwrap_err(),
        FetchError::BadResponse
    );

    let none = serve(|_| {
        Reply::Raw(
            b"HTTP/1.1 302 Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody".to_vec(),
        )
    })
    .await;
    let origin = none.origin();
    let response = local()
        .fetch(get(none.url("/")), |o| o == origin)
        .await
        .unwrap();
    assert_eq!(
        (response.status, response.body.as_slice()),
        (302, &b"body"[..])
    );
}

#[tokio::test]
async fn a_redirect_never_discloses_its_target_or_its_body() {
    let server = serve(|_| redirect(302, "https://internal.example/secret-path")).await;
    let error = local()
        .fetch(get(server.url("/")), |_| false)
        .await
        .unwrap_err();
    // The error is the bare reason; there is nowhere for the location to travel.
    assert_eq!(
        format!("{error} {error:?}"),
        "the destination is not allowed Denied"
    );
}

// ---- private and local destinations ----

#[tokio::test]
async fn local_and_private_destinations_are_refused_before_any_connection() {
    let strict = FetchClient::build(Policy {
        require_https: false,
        allow_local: false,
    })
    .unwrap();
    let server = serve(|_| ok("internal")).await;
    for url in [
        server.url("/"),
        Url::parse(&format!("http://localhost:{}/", server.port)).unwrap(),
        Url::parse(&format!("http://[::1]:{}/", server.port)).unwrap(),
        Url::parse(&format!("http://0.0.0.0:{}/", server.port)).unwrap(),
    ] {
        let error = strict.fetch(get(url.clone()), any).await.unwrap_err();
        assert!(
            matches!(error, FetchError::Denied | FetchError::Unreachable),
            "{url}: {error:?}"
        );
    }
    assert_eq!(server.hits(), 0, "no request reached a local server");
    // A name that resolves inside is refused by the resolver, not by the URL check.
    let by_name = Url::parse(&format!("http://localhost:{}/", server.port)).unwrap();
    assert_eq!(
        strict.fetch(get(by_name), any).await.unwrap_err(),
        FetchError::Denied
    );
}

#[tokio::test]
async fn the_real_client_only_speaks_https() {
    let strict = FetchClient::new().unwrap();
    let server = serve(|_| ok("plain")).await;
    assert_eq!(
        strict.fetch(get(server.url("/")), any).await.unwrap_err(),
        FetchError::Denied
    );
    assert_eq!(server.hits(), 0);
}

#[tokio::test]
async fn an_unreachable_host_is_just_unreachable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };
    let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
    assert_eq!(
        local().fetch(get(url), any).await.unwrap_err(),
        FetchError::Unreachable
    );
}

// ---- response headers ----

#[test]
fn response_headers_are_lowercased_joined_and_bounded() {
    let mut headers = HeaderMap::new();
    headers.append("Set-Cookie", HeaderValue::from_static("a=1"));
    headers.append("Set-Cookie", HeaderValue::from_static("b=2"));
    headers.append("X-One", HeaderValue::from_static("1"));
    let list = header_list(&headers);
    assert!(list.contains(&("set-cookie".to_owned(), "a=1, b=2".to_owned())));
    assert!(list.contains(&("x-one".to_owned(), "1".to_owned())));

    let mut many = HeaderMap::new();
    for index in 0..200 {
        many.insert(
            HeaderName::from_bytes(format!("x-h{index}").as_bytes()).unwrap(),
            HeaderValue::from_static("v"),
        );
    }
    assert_eq!(header_list(&many).len(), MAX_RESPONSE_HEADERS);

    let mut large = HeaderMap::new();
    for index in 0..20 {
        large.insert(
            HeaderName::from_bytes(format!("x-big{index}").as_bytes()).unwrap(),
            HeaderValue::from_str(&"v".repeat(4000)).unwrap(),
        );
    }
    let total: usize = header_list(&large)
        .iter()
        .map(|(n, v)| n.len() + v.len())
        .sum();
    assert!(total <= MAX_RESPONSE_HEADER_BYTES);
}

// ---- the shape of a request ----

fn build(url: &str) -> Result<Request, RequestError> {
    Request::build(url, "", [], Vec::new(), 0, 0)
}

#[test]
fn a_request_needs_an_https_url_without_credentials_and_drops_the_fragment() {
    let request = build("https://Example.com/a?b=1#frag").unwrap();
    assert_eq!(request.url.as_str(), "https://example.com/a?b=1");
    assert_eq!(request.origin().as_deref(), Some("https://example.com"));
    for bad in [
        "http://example.com/",
        "ftp://example.com/",
        "https://user:pw@example.com/",
        "https://user@example.com/",
        "not a url",
        "https://*.example.com/",
        "https://exa mple.com/",
        "",
    ] {
        assert_eq!(build(bad).unwrap_err(), RequestError::Url, "{bad:?}");
    }
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
    assert_eq!(build(&long).unwrap_err(), RequestError::Url);
}

#[test]
fn methods_are_the_usual_six_and_only_those() {
    for (text, method) in [
        ("", Method::Get),
        ("GET", Method::Get),
        ("HEAD", Method::Head),
        ("POST", Method::Post),
        ("PUT", Method::Put),
        ("PATCH", Method::Patch),
        ("DELETE", Method::Delete),
    ] {
        let request = Request::build("https://example.com/", text, [], Vec::new(), 0, 0).unwrap();
        assert_eq!(request.method, method, "{text:?}");
    }
    for bad in [
        "get", "CONNECT", "TRACE", "OPTIONS", "FOO", "GET ", "PROPFIND",
    ] {
        let error = Request::build("https://example.com/", bad, [], Vec::new(), 0, 0).unwrap_err();
        assert_eq!(error, RequestError::Method, "{bad:?}");
    }
}

#[test]
fn headers_are_validated_lowercased_and_the_host_ones_are_refused() {
    let with = |headers: Vec<(&str, &str)>| {
        Request::build("https://example.com/", "POST", headers, b"x".to_vec(), 0, 0)
    };
    let ok = with(vec![
        ("Accept", "text/plain"),
        ("X-Custom", "v"),
        ("Authorization", "Bearer t"),
    ])
    .unwrap();
    assert_eq!(
        ok.headers[0],
        ("accept".to_owned(), "text/plain".to_owned())
    );
    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Upgrade",
        "TE",
        "Expect",
        "Proxy-Authorization",
        "Proxy-Anything",
        "Via",
        "Forwarded",
        "X-Forwarded-For",
    ] {
        assert!(
            matches!(with(vec![(name, "x")]), Err(RequestError::Header(_))),
            "{name}"
        );
    }
    for (name, value) in [
        ("bad name", "x"),
        ("", "x"),
        ("x-a", "line\r\nInjected: yes"),
        ("x-a", "line\nbreak"),
        ("x-a", "nul\0"),
    ] {
        assert!(
            matches!(with(vec![(name, value)]), Err(RequestError::Header(_))),
            "{name:?} {value:?}"
        );
    }
    assert!(matches!(
        with(vec![("x-a", "1"), ("X-A", "2")]),
        Err(RequestError::Header(_))
    ));
    let long = "v".repeat(MAX_REQUEST_HEADER_VALUE_BYTES + 1);
    assert!(matches!(
        with(vec![("x-a", &long)]),
        Err(RequestError::Header(_))
    ));
    let many: Vec<(String, String)> = (0..=MAX_REQUEST_HEADERS)
        .map(|i| (format!("x-{i}"), "v".into()))
        .collect();
    let refs: Vec<(&str, &str)> = many.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    assert!(matches!(with(refs), Err(RequestError::Header(_))));
}

#[test]
fn bodies_are_bounded_and_only_for_methods_that_carry_one() {
    let big = vec![0u8; MAX_REQUEST_BODY_BYTES + 1];
    assert!(matches!(
        Request::build("https://example.com/", "POST", [], big, 0, 0),
        Err(RequestError::Body(_))
    ));
    let limit = vec![0u8; MAX_REQUEST_BODY_BYTES];
    assert!(Request::build("https://example.com/", "PUT", [], limit, 0, 0).is_ok());
    for method in ["GET", "HEAD", ""] {
        assert!(matches!(
            Request::build("https://example.com/", method, [], b"x".to_vec(), 0, 0),
            Err(RequestError::Body(_))
        ));
    }
}

#[test]
fn the_budget_and_the_deadline_default_and_are_capped() {
    let request =
        |bytes, ms| Request::build("https://example.com/", "", [], Vec::new(), bytes, ms).unwrap();
    assert_eq!(request(0, 0).max_response_bytes, DEFAULT_RESPONSE_BYTES);
    assert_eq!(request(0, 0).timeout, MAX_TIMEOUT);
    assert_eq!(request(500, 1500).max_response_bytes, 500);
    assert_eq!(request(500, 1500).timeout, Duration::from_millis(1500));
    assert_eq!(
        request(u64::MAX, u32::MAX).max_response_bytes,
        MAX_RESPONSE_BYTES
    );
    assert_eq!(request(u64::MAX, u32::MAX).timeout, MAX_TIMEOUT);
    assert_eq!(
        request((MAX_RESPONSE_BYTES + 1) as u64, 30_001).max_response_bytes,
        MAX_RESPONSE_BYTES
    );
}
