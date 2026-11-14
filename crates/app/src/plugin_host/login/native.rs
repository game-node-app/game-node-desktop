//! Native Tauri adapter. It does not interpret provider cookies or page results.

use super::{LoginWindow, LoginWindows, MAX_RESULT};
use gamenode_core::ports::BrowserOpener;
use gamenode_plugin_protocol::{
    login::LoginPolicy,
    proto::{LoginCookie, LoginReply},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
    webview::NewWindowResponse,
};
use tonic::Status;

pub struct TauriLoginWindows {
    app: Mutex<Option<AppHandle>>,
    browser: Arc<dyn BrowserOpener>,
}

impl TauriLoginWindows {
    pub fn new(browser: Arc<dyn BrowserOpener>) -> Self {
        Self {
            app: Mutex::new(None),
            browser,
        }
    }

    pub fn attach(&self, app: AppHandle) {
        *self
            .app
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(app);
    }
}

struct NativeWindow {
    app: AppHandle,
    window: WebviewWindow,
    closed: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    close_gate: Arc<Mutex<()>>,
    navigation: Arc<AtomicU64>,
}

#[derive(Debug, PartialEq, Eq)]
enum Navigation {
    Allow,
    External,
    Block,
}

fn navigation(policy: &LoginPolicy, url: &Url) -> Navigation {
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        Navigation::Block
    } else if policy.allows(url) {
        Navigation::Allow
    } else {
        Navigation::External
    }
}

fn external(browser: Arc<dyn BrowserOpener>, url: Url) {
    // Called on the WebView thread. Opening a browser is blocking OS work.
    std::thread::spawn(move || {
        let _ = browser.open(&url);
    });
}

struct ProbeGuard {
    origin: url::Origin,
    generation: u64,
}

impl ProbeGuard {
    fn new(policy: &LoginPolicy, generation: u64) -> Result<Self, Status> {
        let expected = Url::parse(&policy.probe_origin)
            .map_err(|_| Status::failed_precondition("invalid login policy"))?;
        Ok(Self {
            origin: expected.origin(),
            generation,
        })
    }

    fn accepts(&self, url: &Url, generation: u64) -> bool {
        generation == self.generation && url.origin() == self.origin
    }

    fn script(&self, policy: &LoginPolicy) -> Result<String, Status> {
        let origin_json = serde_json::to_string(&self.origin.ascii_serialization())
            .map_err(|_| Status::internal("login policy unavailable"))?;
        Ok(format!(
            "(() => {{ if (window.location.origin !== {origin_json}) return null; return ({}); }})()",
            policy.probe_script.trim().trim_end_matches(';')
        ))
    }
}

#[async_trait::async_trait]
impl LoginWindows for TauriLoginWindows {
    async fn open(
        &self,
        label: String,
        profile: PathBuf,
        policy: Arc<LoginPolicy>,
    ) -> Result<Arc<dyn LoginWindow>, Status> {
        let app = self
            .app
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Status::unavailable("login window unavailable"))?;
        let browser = self.browser.clone();
        tokio::task::spawn_blocking(move || {
            let url = Url::parse(&policy.start_url)
                .map_err(|_| Status::failed_precondition("invalid login policy"))?;
            let closed = Arc::new(AtomicBool::new(false));
            let close_requested = Arc::new(AtomicBool::new(false));
            let close_gate = Arc::new(Mutex::new(()));
            let navigation_epoch = Arc::new(AtomicU64::new(0));
            let epoch = navigation_epoch.clone();
            let navigate_policy = policy.clone();
            let popup_policy = policy.clone();
            let popup_browser = browser.clone();
            let window = WebviewWindowBuilder::new(&app, label, WebviewUrl::External(url))
                .title(&policy.title)
                .inner_size(f64::from(policy.width), f64::from(policy.height))
                .resizable(true)
                .focused(true)
                .data_directory(profile)
                .on_navigation(move |url| {
                    epoch.fetch_add(1, Ordering::AcqRel);
                    match navigation(&navigate_policy, url) {
                        Navigation::Allow => true,
                        Navigation::External => {
                            external(browser.clone(), url.clone());
                            false
                        }
                        Navigation::Block => false,
                    }
                })
                .on_new_window(move |url, _| {
                    if navigation(&popup_policy, &url) == Navigation::External {
                        external(popup_browser.clone(), url);
                    }
                    NewWindowResponse::Deny
                })
                .build()
                .map_err(|_| Status::unavailable("login window could not open"))?;
            let flag = closed.clone();
            let requested = close_requested.clone();
            let gate = close_gate.clone();
            let close_window = window.clone();
            window.on_window_event(move |event| {
                match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        // cookies_for_url waits synchronously for a reply from the UI thread.
                        // Let that request finish before destroying its WebView, otherwise Wry
                        // can drop the reply channel and panic in its recv().unwrap().
                        if !requested.swap(true, Ordering::AcqRel) {
                            api.prevent_close();
                            let deferred_window = close_window.clone();
                            let deferred_gate = gate.clone();
                            std::thread::spawn(move || {
                                let _guard = deferred_gate
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                let _ = deferred_window.close();
                            });
                        }
                    }
                    WindowEvent::Destroyed => flag.store(true, Ordering::Release),
                    _ => {}
                }
            });
            Ok(Arc::new(NativeWindow {
                app,
                window,
                closed,
                close_requested,
                close_gate,
                navigation: navigation_epoch,
            }) as Arc<dyn LoginWindow>)
        })
        .await
        .map_err(|_| Status::unavailable("login window could not open"))?
    }
}

async fn worker<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, Status> + Send + 'static,
) -> Result<T, Status> {
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|_| Status::unavailable("login window unavailable"))?
}

#[async_trait::async_trait]
impl LoginWindow for NativeWindow {
    async fn focus(&self) -> Result<(), Status> {
        let window = self.window.clone();
        worker(move || {
            window
                .unminimize()
                .map_err(|_| Status::unavailable("login focus failed"))?;
            window
                .show()
                .map_err(|_| Status::unavailable("login focus failed"))?;
            window
                .set_focus()
                .map_err(|_| Status::unavailable("login focus failed"))
        })
        .await
    }

    async fn close(&self) -> Result<(), Status> {
        let window = self.window.clone();
        let app = self.app.clone();
        let close_requested = self.close_requested.clone();
        let close_gate = self.close_gate.clone();
        let label = window.label().to_owned();
        worker(move || {
            if app.get_webview_window(&label).is_some() {
                close_requested.store(true, Ordering::Release);
                let _guard = close_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                window
                    .close()
                    .map_err(|_| Status::unavailable("login close failed"))?;
            }
            Ok(())
        })
        .await?;
        for _ in 0..40 {
            if self.app.get_webview_window(self.window.label()).is_none() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(Status::deadline_exceeded("login close timed out"))
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    async fn collect(&self, policy: Arc<LoginPolicy>) -> Result<LoginReply, Status> {
        let guard = Arc::new(ProbeGuard::new(
            &policy,
            self.navigation.load(Ordering::Acquire),
        )?);
        let window = self.window.clone();
        let check = window.clone();
        let check_guard = guard.clone();
        let navigation = self.navigation.clone();
        if !worker(move || {
            Ok(check
                .url()
                .is_ok_and(|url| check_guard.accepts(&url, navigation.load(Ordering::Acquire))))
        })
        .await?
        {
            return Ok(LoginReply::default());
        }
        // Check again inside the document, since evaluation is queued by Tauri.
        let script = guard.script(&policy)?;
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        worker(move || {
            window
                .eval_with_callback(script, move |value| {
                    let _ = sender.try_send(if value.len() <= MAX_RESULT {
                        Some(value)
                    } else {
                        None
                    });
                })
                .map_err(|_| Status::unavailable("login collection unavailable"))
        })
        .await?;
        let page = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .map_err(|_| Status::deadline_exceeded("login probe timed out"))?
            .ok_or_else(|| Status::unavailable("login collection unavailable"))?
            .ok_or_else(|| Status::resource_exhausted("login result limit exceeded"))?;
        let cookies = if policy.cookie_names.is_empty() {
            // Probe-only policies (for example an authorization-code flow) declare no cookies.
            Vec::new()
        } else {
            let window = self.window.clone();
            let close_gate = self.close_gate.clone();
            let close_requested = self.close_requested.clone();
            let cookie_policy = policy.clone();
            let cookies = worker(move || {
                let _guard = close_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if close_requested.load(Ordering::Acquire) {
                    return Ok(None);
                }
                let mut collected: Vec<LoginCookie> = Vec::new();
                for cookie_url in &cookie_policy.cookie_urls {
                    let cookie_url = Url::parse(cookie_url)
                        .map_err(|_| Status::failed_precondition("invalid login policy"))?;
                    let stored = window
                        .cookies_for_url(cookie_url)
                        .map_err(|_| Status::unavailable("login cookies unavailable"))?;
                    for name in &cookie_policy.cookie_names {
                        // One physical cookie per name: the first URL that exposes it wins.
                        if collected.iter().any(|cookie| cookie.name == *name) {
                            continue;
                        }
                        if let Some(cookie) = stored
                            .iter()
                            .find(|cookie| cookie.name() == name && !cookie.value().is_empty())
                        {
                            collected.push(LoginCookie {
                                name: cookie.name().to_owned(),
                                value: cookie.value().to_owned(),
                            });
                        }
                    }
                }
                Ok(Some(collected))
            })
            .await?;
            let Some(cookies) = cookies else {
                return Ok(LoginReply::default());
            };
            cookies
        };
        let check = self.window.clone();
        let check_guard = guard.clone();
        let navigation = self.navigation.clone();
        if !worker(move || {
            Ok(check
                .url()
                .is_ok_and(|url| check_guard.accepts(&url, navigation.load(Ordering::Acquire))))
        })
        .await?
            || self.navigation.load(Ordering::Acquire) != guard.generation
        {
            return Ok(LoginReply::default());
        }
        Ok(LoginReply {
            page_result_json: if page.trim() == "null" {
                None
            } else {
                Some(page)
            },
            cookies,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_popup_policy_is_provider_neutral() {
        let policy: LoginPolicy =
            toml::from_str(include_str!("../../../../plugin-steam/assets/login.toml")).unwrap();
        for url in [
            "https://store.steampowered.com/login/",
            "https://help.steampowered.com/en/",
            "https://steamcommunity.com/login/",
            "https://cdn.steamstatic.com/a",
        ] {
            assert_eq!(
                navigation(&policy, &Url::parse(url).unwrap()),
                Navigation::Allow
            );
        }
        for url in [
            "https://example.com/",
            "https://steampowered.com.example.com/",
        ] {
            assert_eq!(
                navigation(&policy, &Url::parse(url).unwrap()),
                Navigation::External
            );
        }
        for url in [
            "http://store.steampowered.com/",
            "javascript:alert(1)",
            "steam://run/620",
            "https://user:secret@store.steampowered.com/",
        ] {
            assert_eq!(
                navigation(&policy, &Url::parse(url).unwrap()),
                Navigation::Block
            );
        }
    }
    #[test]
    fn probe_requires_exact_origin_and_unchanged_navigation_even_after_returning() {
        let policy: LoginPolicy =
            toml::from_str(include_str!("../../../../plugin-steam/assets/login.toml")).unwrap();
        let guard = ProbeGuard::new(&policy, 7).unwrap();
        let original = Url::parse("https://store.steampowered.com/explore/").unwrap();
        assert!(guard.accepts(&original, 7));
        for other in [
            "https://store.steampowered.com:8443/",
            "https://login.steampowered.com/",
            "http://store.steampowered.com/",
            "https://example.com/",
        ] {
            assert!(!guard.accepts(&Url::parse(other).unwrap(), 7));
        }
        assert!(
            !guard.accepts(&original, 8),
            "returning to the origin cannot make an old collection current"
        );
        let script = guard.script(&policy).unwrap();
        assert!(
            script.find("window.location.origin").unwrap()
                < script.find("application_config").unwrap()
        );
    }
}
