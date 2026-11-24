//! Notifications a plugin asks Core to show, through `Notifications.Publish`.
//!
//! A notification is plain text attributed to the plugin that sent it: the attribution comes from
//! the authenticated session, never from the request. The host bounds the text, refuses anything
//! that could disguise it (control characters, bidirectional overrides, invisible separators),
//! limits how often a plugin may speak, and hands the result to a [`NotificationSink`], which the
//! application connects to its window. The host never interprets the text: it is not markup, a
//! link or an action, and the UI renders it as plain text.
//!
//! Notifications are events, not state: one that is delivered while no window listens is lost.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The Tauri event a delivered notice leaves on. The UI listens to the same name.
pub const EVENT: &str = "plugin:notification";

/// The longest title, in bytes.
pub const MAX_TITLE_BYTES: usize = 120;
/// The longest body, in bytes, and the most lines it may have.
pub const MAX_BODY_BYTES: usize = 512;
pub const MAX_BODY_LINES: usize = 8;
/// The most translations beside the main text.
pub const MAX_TRANSLATIONS: usize = 8;
/// The longest language tag.
pub const MAX_LOCALE_BYTES: usize = 35;
/// How many notifications one plugin may publish in [`WINDOW`].
pub const PER_PLUGIN_PER_WINDOW: usize = 5;
/// How many notifications all plugins together may publish in [`WINDOW`].
pub const GLOBAL_PER_WINDOW: usize = 20;
/// The length of the rate-limit window.
pub const WINDOW: Duration = Duration::from_secs(60);

/// How much a notification matters, as the plugin names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "" | "info" => Self::Info,
            "success" => Self::Success,
            "warning" => Self::Warning,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// One version of a notification's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    /// A language tag, or empty when the plugin gave none.
    pub locale: String,
    pub title: String,
    pub body: String,
}

/// What the application is told to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginNotice {
    /// The authenticated plugin id that sent it.
    pub plugin_id: String,
    pub severity: Severity,
    /// The plugin's main text first, then its translations.
    pub texts: Vec<Text>,
}

/// The application's side: shows a notification. Called from a service thread, so it must hand
/// the notice off and return.
pub trait NotificationSink: Send + Sync {
    fn deliver(&self, notice: PluginNotice);
}

/// Characters that would let a text pretend to be something else: bidirectional controls,
/// invisible separators and tags. Ordinary format characters that real text needs (the joiners
/// of emoji and several scripts) are allowed.
fn disguises(c: char) -> bool {
    matches!(c,
        '\u{061C}' | '\u{200B}' | '\u{200E}' | '\u{200F}'
        | '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}')
}

/// Whether `text` is plain text on one line: no control characters and none that disguise text.
pub fn is_plain_line(text: &str) -> bool {
    clean(text, false)
}

fn clean(text: &str, multiline: bool) -> bool {
    text.chars()
        .all(|c| !disguises(c) && (!c.is_control() || (multiline && c == '\n')))
}

/// Why a notification was refused. The messages are safe to show the plugin.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Refused(pub String);

fn refused(message: impl Into<String>) -> Refused {
    Refused(message.into())
}

/// A language tag as far as the host checks one: letters, digits and hyphens.
fn valid_locale(locale: &str) -> bool {
    locale.len() <= MAX_LOCALE_BYTES
        && locale
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !locale.starts_with('-')
        && !locale.ends_with('-')
        && !locale.contains("--")
}

/// Checks one version of the text and returns it trimmed.
pub fn check_text(locale: &str, title: &str, body: &str) -> Result<Text, Refused> {
    if !valid_locale(locale) {
        return Err(refused(
            "locale must be a language tag of letters, digits and hyphens, such as \"en\" or \"pt-BR\"",
        ));
    }
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() {
        return Err(refused("title is required"));
    }
    if title.len() > MAX_TITLE_BYTES {
        return Err(refused(format!(
            "title is longer than {MAX_TITLE_BYTES} bytes"
        )));
    }
    if !clean(title, false) {
        return Err(refused(
            "title must be plain text on one line: no control characters, bidirectional overrides or invisible separators",
        ));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(refused(format!(
            "body is longer than {MAX_BODY_BYTES} bytes"
        )));
    }
    if !clean(body, true) {
        return Err(refused(
            "body must be plain text: no control characters other than line breaks, bidirectional overrides or invisible separators",
        ));
    }
    if body.lines().count() > MAX_BODY_LINES {
        return Err(refused(format!(
            "body has more than {MAX_BODY_LINES} lines"
        )));
    }
    Ok(Text {
        locale: locale.to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
    })
}

/// A sliding window of the times something was admitted.
#[derive(Default)]
struct Window(VecDeque<Instant>);

impl Window {
    fn prune(&mut self, now: Instant) {
        while self
            .0
            .front()
            .is_some_and(|at| now.duration_since(*at) >= WINDOW)
        {
            self.0.pop_front();
        }
    }

    /// How long until there is room for one more under `limit`, or `None` if there is room now.
    fn wait(&self, now: Instant, limit: usize) -> Option<Duration> {
        (self.0.len() >= limit).then(|| {
            let oldest = self.0[self.0.len() - limit];
            WINDOW.saturating_sub(now.duration_since(oldest))
        })
    }
}

#[derive(Default)]
struct Windows {
    global: Window,
    per_plugin: HashMap<[u8; 32], Window>,
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The notification service shared by every plugin session: the sink and the rate limits.
pub struct Notifications {
    sink: Mutex<Option<Arc<dyn NotificationSink>>>,
    windows: Mutex<Windows>,
    clock: Clock,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            sink: Mutex::default(),
            windows: Mutex::default(),
            clock: Arc::new(Instant::now),
        }
    }
}

impl Notifications {
    /// Connects the application's side. Until it is, publishing is unavailable.
    pub fn set_sink(&self, sink: Arc<dyn NotificationSink>) {
        *self.sink.lock().unwrap_or_else(|p| p.into_inner()) = Some(sink);
    }

    pub(crate) fn sink(&self) -> Option<Arc<dyn NotificationSink>> {
        self.sink.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Replaces the clock, for tests of the rate limit.
    #[cfg(test)]
    pub(crate) fn with_clock(
        mut self,
        clock: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Counts one notification of `owner` against the limits, or says how long to wait. A
    /// refusal counts nothing.
    pub(crate) fn admit(&self, owner: [u8; 32]) -> Result<(), Duration> {
        let now = (self.clock)();
        let mut windows = self.windows.lock().unwrap_or_else(|p| p.into_inner());
        windows.global.prune(now);
        windows.per_plugin.retain(|_, window| {
            window.prune(now);
            !window.0.is_empty()
        });
        let own = windows.per_plugin.get(&owner);
        let wait = [
            own.and_then(|window| window.wait(now, PER_PLUGIN_PER_WINDOW)),
            windows.global.wait(now, GLOBAL_PER_WINDOW),
        ]
        .into_iter()
        .flatten()
        .max();
        if let Some(wait) = wait {
            return Err(wait.max(Duration::from_secs(1)));
        }
        windows.global.0.push_back(now);
        windows
            .per_plugin
            .entry(owner)
            .or_default()
            .0
            .push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
