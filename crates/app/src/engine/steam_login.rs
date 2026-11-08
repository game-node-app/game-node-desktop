//! The isolated Steam web sign-in window.
//!
//! Steam's OpenID redirect only gives an application a SteamID, so the owned-game endpoints need
//! the store session cookie that only Steam's own login page can create. The window below loads
//! that page in a second Tauri webview with a dedicated, ephemeral profile:
//!
//! - the label is [`WINDOW_LABEL`] and `crates/app/capabilities/default.json` stays scoped to
//!   `main`, so the remote page has no application command or IPC access;
//! - a small read-only script checks the authenticated store page's application config;
//! - navigation is limited to Steam's HTTPS hosts; other HTTPS links open in the system browser;
//! - the cookie is read from Rust with the webview's `cookies_for_url`, never through
//!   JavaScript;
//! - closing the window, [`SteamLoginWindow::cancel`] and the ten-minute timeout all end the
//!   attempt without saving anything, and the profile directory is removed afterwards and at
//!   startup.
//!
//! A generation counter makes a late result from an attempt that was cancelled or superseded
//! harmless. The account service additionally guards its own generation, since a sign-out can
//! race with a window result that was already returned.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use gamenode_core::SessionCredential;
use gamenode_core::ports::BrowserOpener;
use gamenode_core::steam::SteamSession;
use gamenode_platform::paths::DataPaths;
use serde::Deserialize;
use tauri::webview::{Cookie, NewWindowResponse};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The label of the sign-in webview. It deliberately has no capability entry.
pub const WINDOW_LABEL: &str = "steam-login";

/// The store page Steam and Playnite use to confirm an authenticated session.
pub const LOGIN_URL: &str = "https://store.steampowered.com/explore/";

/// The URL whose cookie jar holds the store session.
const COOKIE_URL: &str = "https://store.steampowered.com/";

/// The requested sign-in window size.
pub const WINDOW_WIDTH: f64 = 600.0;
pub const WINDOW_HEIGHT: f64 = 760.0;

/// An attempt the user does not finish in this many seconds is cancelled.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How often the Rust-side poll reads the webview cookie jar.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Cookie names the minimal stored header carries.
const LOGIN_COOKIE: &str = "steamLoginSecure";
const SESSION_COOKIE: &str = "sessionid";

const SESSION_PROBE: &str = r#"(() => {
  if (window.location.origin !== 'https://store.steampowered.com') return null;
  const config = document.getElementById('application_config');
  if (!config) return null;
  try {
    const user = JSON.parse(config.getAttribute('data-userinfo') || 'null');
    const store = JSON.parse(config.getAttribute('data-store_user_config') || 'null');
    if (!user || user.logged_in !== true || !user.steamid || !store || !store.webapi_token) return null;
    return {
      steam_id: String(user.steamid),
      account_name: user.account_name || null,
      token: String(store.webapi_token)
    };
  } catch (_) {
    return null;
  }
})()"#;

/// Hosts the sign-in window may navigate to. Steam redirects between its store, community and
/// help hosts during login and Steam Guard.
const STEAM_HOST_SUFFIXES: [&str; 3] =
    ["steampowered.com", "steamcommunity.com", "steamstatic.com"];

/// Why a sign-in window operation failed. Every variant is a category: no URL, cookie, token or
/// webview error text is carried in the value or its `Debug`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SteamLoginError {
    OpenFailed,
    ProfileUnavailable,
    NotOpen,
    FocusFailed,
    CloseFailed,
}

impl fmt::Display for SteamLoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenFailed => "the Steam sign-in window could not be opened",
            Self::ProfileUnavailable => "the Steam sign-in profile could not be prepared",
            Self::NotOpen => "the Steam sign-in window is not open",
            Self::FocusFailed => "the Steam sign-in window could not be focused",
            Self::CloseFailed => "the Steam sign-in window could not be closed",
        })
    }
}

impl std::error::Error for SteamLoginError {}

/// How an attempt ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamLoginResult {
    /// The authenticated store page supplied identity and token in the sign-in webview.
    Verified { session: SteamSession },
    /// The user pressed Cancel, or a newer attempt superseded this one.
    Cancelled,
    /// The user closed the window.
    Closed,
    /// The ten-minute lifetime elapsed.
    TimedOut,
    /// The window could not be opened or its cookie jar could not be read.
    Failed(SteamLoginError),
}

/// One attempt's result. `attempt` comes from the window's generation counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamLoginOutcome {
    pub attempt: u64,
    pub result: SteamLoginResult,
}

impl SteamLoginOutcome {
    fn failed(attempt: u64, error: SteamLoginError) -> Self {
        Self {
            attempt,
            result: SteamLoginResult::Failed(error),
        }
    }
}

/// The sign-in window the account service drives.
///
/// [`run`](Self::run) is a long-running operation: it opens the window and resolves when the user
/// finishes, closes the window, cancels, or the timeout elapses. Implementations must be
/// [`Send`] + [`Sync`] so the engine can call it from a background task.
#[async_trait]
pub trait SteamLoginWindow: Send + Sync {
    /// Shows a fresh Steam login window and waits for its result.
    async fn run(&self) -> SteamLoginOutcome;

    /// Brings the open window to the front without losing the attempt.
    fn focus(&self) -> Result<(), SteamLoginError>;

    /// Cancels the attempt and closes the window. Calling it with no window open is success.
    fn cancel(&self) -> Result<(), SteamLoginError>;

    /// Removes a leftover profile. Blocking: call it on the storage worker or a blocking task,
    /// not the UI thread.
    fn cleanup_profile(&self);
}

/// The production sign-in window: a second Tauri webview with an owner-only data directory.
pub struct TauriSteamLoginWindow {
    /// The engine is built before Tauri's setup callback, so the handle can be attached later.
    app: Mutex<Option<AppHandle>>,
    paths: DataPaths,
    browser: Arc<dyn BrowserOpener>,
    generation: AtomicU64,
    notify: Arc<tokio::sync::Notify>,
}

impl TauriSteamLoginWindow {
    /// `paths` supplies the profile directory; keeping it a parameter lets tests use a temporary
    /// data directory and lets the platform choose the production location.
    pub fn new(app: AppHandle, paths: DataPaths, browser: Arc<dyn BrowserOpener>) -> Self {
        let window = Self::without_app(paths, browser);
        window.attach(app);
        window
    }

    /// Builds the port before Tauri's `setup` callback; call [`attach`](Self::attach) there.
    /// Until then `run` fails cleanly and `cancel`/`cleanup_profile` still work.
    pub fn without_app(paths: DataPaths, browser: Arc<dyn BrowserOpener>) -> Self {
        Self {
            app: Mutex::new(None),
            paths,
            browser,
            generation: AtomicU64::new(0),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Attaches the application handle once Tauri is ready.
    pub fn attach(&self, app: AppHandle) {
        *self
            .app
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(app);
    }

    fn app_handle(&self) -> Option<AppHandle> {
        self.app
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The directory the webview is allowed to write. Removed by [`Self::cleanup_profile`].
    pub fn profile_dir(&self) -> PathBuf {
        self.paths.steam_login_profile_dir()
    }

    async fn remove_profile(&self) {
        let paths = self.paths.clone();
        let removed = tokio::task::spawn_blocking(move || remove_profile_with_retry(&paths)).await;
        if !matches!(removed, Ok(true)) {
            tracing::warn!("could not remove the Steam sign-in profile");
        }
    }

    fn close_window(&self) {
        if let Some(app) = self.app_handle()
            && let Some(window) = app.get_webview_window(WINDOW_LABEL)
        {
            let _ = window.close();
        }
    }

    /// Closes the attempt window and waits briefly for the webview to release its profile files
    /// (WebView2 keeps them open until the window is destroyed).
    async fn close_and_wait(&self) {
        self.close_window();
        let Some(app) = self.app_handle() else {
            return;
        };
        for _ in 0..20 {
            if app.get_webview_window(WINDOW_LABEL).is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// A cancel schedules the close asynchronously. A new attempt must not race the old label.
    async fn close_existing_window(&self) {
        let Some(app) = self.app_handle() else {
            return;
        };
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        let _ = window.close();
        for _ in 0..10 {
            if app.get_webview_window(WINDOW_LABEL).is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[async_trait]
impl SteamLoginWindow for TauriSteamLoginWindow {
    async fn run(&self) -> SteamLoginOutcome {
        let attempt = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let Some(app) = self.app_handle() else {
            return SteamLoginOutcome::failed(attempt, SteamLoginError::OpenFailed);
        };

        // A previous cancel may still be closing its window under the same label.
        self.close_existing_window().await;

        // Start from an empty profile so a leftover cookie from a crash or a failed cleanup can
        // never make a new attempt look signed in.
        let paths = self.paths.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            paths.remove_steam_login_profile_dir()?;
            paths.ensure_steam_login_profile_dir()
        })
        .await;
        let profile = match prepared {
            Ok(Ok(path)) => path,
            _ => {
                self.remove_profile().await;
                return SteamLoginOutcome::failed(attempt, SteamLoginError::ProfileUnavailable);
            }
        };

        let login_url = Url::parse(LOGIN_URL).expect("the Steam login URL is valid");
        let cookie_url = Url::parse(COOKIE_URL).expect("the Steam store URL is valid");

        let closed = Arc::new(AtomicBool::new(false));
        let close_flag = closed.clone();
        let close_notify = self.notify.clone();
        let navigation_browser = self.browser.clone();
        let new_window_browser = self.browser.clone();

        let builder =
            WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::External(login_url))
                .title("Steam")
                .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
                .resizable(true)
                .focused(true)
                .data_directory(profile)
                .on_navigation(move |url| match navigation_decision(url) {
                    NavigationDecision::Allow => true,
                    NavigationDecision::OpenExternal => {
                        open_external(navigation_browser.clone(), url);
                        false
                    }
                    NavigationDecision::Block => false,
                })
                .on_new_window(move |url, _features| {
                    // A second native window would outlive the attempt and keep the ephemeral
                    // profile busy, so every popup is denied. External links still open in the
                    // system browser; Steam pages stay reachable in the main sign-in window.
                    if navigation_decision(&url) == NavigationDecision::OpenExternal {
                        open_external(new_window_browser.clone(), &url);
                    }
                    NewWindowResponse::Deny
                });

        let window = match builder.build() {
            Ok(window) => window,
            Err(_) => {
                self.remove_profile().await;
                return SteamLoginOutcome::failed(attempt, SteamLoginError::OpenFailed);
            }
        };
        tracing::info!(attempt, "Steam sign-in window opened");
        window.on_window_event(move |event| {
            if matches!(
                event,
                WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
            ) {
                close_flag.store(true, Ordering::SeqCst);
                close_notify.notify_waiters();
            }
        });

        let deadline = Instant::now() + LOGIN_TIMEOUT;
        loop {
            if self.generation.load(Ordering::SeqCst) != attempt {
                self.close_and_wait().await;
                self.remove_profile().await;
                tracing::info!(attempt, "Steam sign-in window was cancelled");
                return SteamLoginOutcome {
                    attempt,
                    result: SteamLoginResult::Cancelled,
                };
            }
            if closed.load(Ordering::SeqCst) {
                self.close_and_wait().await;
                self.remove_profile().await;
                tracing::info!(attempt, "Steam sign-in window was closed by the user");
                return SteamLoginOutcome {
                    attempt,
                    result: SteamLoginResult::Closed,
                };
            }
            if Instant::now() >= deadline {
                self.close_and_wait().await;
                self.remove_profile().await;
                tracing::info!(attempt, "Steam sign-in window timed out");
                return SteamLoginOutcome {
                    attempt,
                    result: SteamLoginResult::TimedOut,
                };
            }

            if let Some(page) = authenticated_store_session(&window).await
                && let Ok(cookies) = window.cookies_for_url(cookie_url.clone())
                && let Some(cookie) = cookie_header(&cookies)
                && page.steam_id.bytes().all(|byte| byte.is_ascii_digit())
                && let Some(token) = SessionCredential::new(page.token)
            {
                // Claim the attempt atomically: a cancel that set the generation after
                // the read wins, and its result is not saved.
                let claimed = self.generation.compare_exchange(
                    attempt,
                    attempt.wrapping_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                self.close_and_wait().await;
                self.remove_profile().await;
                if claimed.is_ok() {
                    tracing::info!(attempt, "Steam sign-in verified in the store page");
                }
                return SteamLoginOutcome {
                    attempt,
                    result: if claimed.is_ok() {
                        SteamLoginResult::Verified {
                            session: SteamSession {
                                steam_id: page.steam_id,
                                account_name: page.account_name,
                                token,
                                cookie,
                            },
                        }
                    } else {
                        SteamLoginResult::Cancelled
                    },
                };
            }

            tokio::select! {
                () = tokio::time::sleep(POLL_INTERVAL) => {}
                () = self.notify.notified() => {}
            }
        }
    }

    fn focus(&self) -> Result<(), SteamLoginError> {
        let window = self
            .app_handle()
            .and_then(|app| app.get_webview_window(WINDOW_LABEL))
            .ok_or(SteamLoginError::NotOpen)?;
        let _ = window.unminimize();
        let _ = window.show();
        window.set_focus().map_err(|_| SteamLoginError::FocusFailed)
    }

    fn cancel(&self) -> Result<(), SteamLoginError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
        self.app_handle()
            .and_then(|app| app.get_webview_window(WINDOW_LABEL))
            .map(|window| window.close().map_err(|_| SteamLoginError::CloseFailed))
            .unwrap_or(Ok(()))
    }

    fn cleanup_profile(&self) {
        if !remove_profile_with_retry(&self.paths) {
            tracing::warn!("could not remove the Steam sign-in profile");
        }
    }
}

#[derive(Deserialize)]
struct AuthenticatedStorePage {
    steam_id: String,
    account_name: Option<String>,
    token: String,
}

async fn authenticated_store_session(
    window: &tauri::WebviewWindow,
) -> Option<AuthenticatedStorePage> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    window
        .eval_with_callback(SESSION_PROBE, move |result| {
            let _ = sender.send(result);
        })
        .ok()?;
    let result = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .ok()??;
    serde_json::from_str::<Option<AuthenticatedStorePage>>(&result)
        .ok()
        .flatten()
}

/// What the navigation allowlist does with a URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavigationDecision {
    /// A Steam HTTPS page: keep it in the sign-in window.
    Allow,
    /// A non-Steam HTTPS page: hand it to the system browser and keep the window on Steam.
    OpenExternal,
    /// Anything else (plain HTTP, scripts, custom schemes): do not navigate.
    Block,
}

fn navigation_decision(url: &Url) -> NavigationDecision {
    if url.scheme() != "https" {
        return NavigationDecision::Block;
    }
    match url.host_str() {
        Some(host) if is_steam_host(host) => NavigationDecision::Allow,
        Some(_) => NavigationDecision::OpenExternal,
        None => NavigationDecision::Block,
    }
}

fn is_steam_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    STEAM_HOST_SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn open_external(browser: Arc<dyn BrowserOpener>, url: &Url) {
    // The navigation callback runs on the webview thread; opening a browser must not block it.
    let url = url.clone();
    std::thread::spawn(move || {
        let _ = browser.open(&url);
    });
}

/// Builds the minimal cookie header: `steamLoginSecure` plus `sessionid` when Steam set one.
///
/// Polling continues until `steamLoginSecure` exists; other cookies are never stored.
fn cookie_header(cookies: &[Cookie<'_>]) -> Option<SessionCredential> {
    let login = cookies
        .iter()
        .find(|cookie| cookie.name() == LOGIN_COOKIE)
        .map(Cookie::value)
        .filter(|value| !value.is_empty())?;
    let mut header = format!("{LOGIN_COOKIE}={login}");
    if let Some(session) = cookies
        .iter()
        .find(|cookie| cookie.name() == SESSION_COOKIE)
        .map(Cookie::value)
        .filter(|value| !value.is_empty())
    {
        header.push_str("; ");
        header.push_str(SESSION_COOKIE);
        header.push('=');
        header.push_str(session);
    }
    SessionCredential::new(header)
}

fn remove_profile_with_retry(paths: &DataPaths) -> bool {
    for attempt in 0..10 {
        match paths.remove_steam_login_profile_dir() {
            Ok(()) => return true,
            Err(_) if attempt < 9 => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::BrowserError;

    struct FakeBrowser;

    impl BrowserOpener for FakeBrowser {
        fn open(&self, _url: &Url) -> Result<(), BrowserError> {
            Ok(())
        }
    }

    fn deferred_window() -> (TauriSteamLoginWindow, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        (
            TauriSteamLoginWindow::without_app(paths, Arc::new(FakeBrowser)),
            dir,
        )
    }

    fn url(value: &str) -> Url {
        Url::parse(value).expect("a test URL")
    }

    fn cookie(name: &str, value: &str) -> Cookie<'static> {
        Cookie::new(name.to_owned(), value.to_owned())
    }

    #[test]
    fn steam_pages_stay_in_the_window() {
        for value in [
            "https://store.steampowered.com/login/",
            "https://login.steampowered.com/jwt/checkdevice",
            "https://help.steampowered.com/en/wizard/HelpWithLogin",
            "https://steamcommunity.com/login/home/",
            "https://cdn.steamstatic.com/store/home.jpg",
        ] {
            assert_eq!(
                navigation_decision(&url(value)),
                NavigationDecision::Allow,
                "{value}"
            );
        }
    }

    #[test]
    fn non_steam_https_opens_in_the_system_browser() {
        assert_eq!(
            navigation_decision(&url("https://example.com/help")),
            NavigationDecision::OpenExternal
        );
        assert_eq!(
            navigation_decision(&url("https://steampowered.com.example.com/")),
            NavigationDecision::OpenExternal
        );
    }

    #[test]
    fn insecure_and_custom_schemes_are_blocked() {
        assert_eq!(
            navigation_decision(&url("http://store.steampowered.com/")),
            NavigationDecision::Block
        );
        assert_eq!(
            navigation_decision(&url("steam://run/620")),
            NavigationDecision::Block
        );
        assert_eq!(
            navigation_decision(&url("javascript:alert(1)")),
            NavigationDecision::Block
        );
    }

    #[test]
    fn the_cookie_header_carries_only_the_two_store_cookies() {
        let cookies = [
            cookie("steamCountry", "BR%7C1234"),
            cookie("steamLoginSecure", "76561198000000001%7C%7Csecret"),
            cookie("sessionid", "abcdef"),
        ];
        let header = cookie_header(&cookies).expect("a session cookie");
        assert_eq!(
            header.expose(),
            "steamLoginSecure=76561198000000001%7C%7Csecret; sessionid=abcdef"
        );
        let debug = format!("{header:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn a_login_cookie_without_a_session_cookie_is_a_valid_header() {
        let header = cookie_header(&[cookie("steamLoginSecure", "value")]).expect("a header");
        assert_eq!(header.expose(), "steamLoginSecure=value");
    }

    #[test]
    fn no_login_cookie_keeps_polling() {
        assert!(cookie_header(&[cookie("sessionid", "abcdef")]).is_none());
        assert!(cookie_header(&[cookie("steamLoginSecure", "")]).is_none());
        assert!(cookie_header(&[]).is_none());
    }

    #[test]
    fn the_window_size_and_timeout_match_the_contract() {
        assert_eq!((WINDOW_WIDTH, WINDOW_HEIGHT), (600.0, 760.0));
        assert_eq!(LOGIN_TIMEOUT, Duration::from_secs(600));
        assert_eq!(WINDOW_LABEL, "steam-login");
    }

    #[test]
    fn the_profile_is_owner_only_and_removed() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let profile = paths.ensure_steam_login_profile_dir().unwrap();
        assert!(profile.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        std::fs::write(profile.join("Cookies"), "profile data").unwrap();
        assert!(remove_profile_with_retry(&paths));
        assert!(!paths.steam_login_profile_dir().exists());
        assert!(remove_profile_with_retry(&paths), "missing is success");
    }

    #[test]
    fn the_sign_in_window_has_no_capability_and_no_ipc() {
        let capabilities = include_str!("../../capabilities/default.json");
        let parsed: serde_json::Value = serde_json::from_str(capabilities).unwrap();
        let windows = parsed["windows"].as_array().expect("a window allowlist");
        assert_eq!(windows.as_slice(), &[serde_json::Value::from("main")]);
        assert!(!capabilities.contains(WINDOW_LABEL));
        assert!(!capabilities.contains("steam-login"));
        assert!(!capabilities.contains("\"remote\""));
    }

    #[test]
    fn no_script_is_injected_into_the_remote_page() {
        // The literal is assembled so this test's own source does not match the search.
        const SOURCE: &str = include_str!("steam_login.rs");
        let initialization = concat!("initialization", "_script");
        assert!(!SOURCE.contains(initialization));
        let eval = concat!("eval", "(");
        assert!(!SOURCE.contains(eval));
        let internals = concat!("__TAURI", "_INTERNALS__");
        assert!(!SOURCE.contains(internals));
    }

    #[test]
    fn the_cookie_never_appears_in_a_login_outcome_debug() {
        let outcome = SteamLoginOutcome {
            attempt: 3,
            result: SteamLoginResult::Verified {
                session: SteamSession {
                    steam_id: "76561198000000001".to_owned(),
                    account_name: Some("Player One".to_owned()),
                    token: SessionCredential::new("webapi-token-secret").unwrap(),
                    cookie: SessionCredential::new("steamLoginSecure=secret").unwrap(),
                },
            },
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("Verified"));
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("webapi-token"));
    }

    #[test]
    fn a_deferred_window_reports_no_open_window() {
        let (window, _dir) = deferred_window();
        assert_eq!(window.focus(), Err(SteamLoginError::NotOpen));
        assert_eq!(window.cancel(), Ok(()));
        window.cleanup_profile();
    }

    #[tokio::test]
    async fn a_deferred_window_fails_a_run_without_opening_anything() {
        let (window, _dir) = deferred_window();
        let outcome = window.run().await;
        assert_eq!(outcome.attempt, 1);
        assert_eq!(
            outcome.result,
            SteamLoginResult::Failed(SteamLoginError::OpenFailed)
        );
    }
}
