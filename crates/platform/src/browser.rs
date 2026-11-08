//! Opens URLs in the user's default browser.

use gamenode_core::ports::{BrowserError, BrowserOpener};
use url::Url;

/// The system browser (`xdg-open` on Linux, `ShellExecute` on Windows). Only `http(s)` URLs are
/// opened, and the URL is passed as a single argument, never through a shell.
pub struct SystemBrowser;

impl BrowserOpener for SystemBrowser {
    fn open(&self, url: &Url) -> Result<(), BrowserError> {
        if !is_web_url(url) {
            return Err(BrowserError);
        }
        open::that_detached(url.as_str()).map_err(|error| {
            tracing::warn!(kind = ?error.kind(), "could not open the system browser");
            BrowserError
        })
    }
}

fn is_web_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host().is_some()
}

/// Shows a directory in the OS file manager. Used by Settings to open the log folder.
pub fn open_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    open::that_detached(path).inspect_err(|error| {
        tracing::warn!(kind = ?error.kind(), "could not open a directory");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_urls_are_openable() {
        assert!(is_web_url(
            &Url::parse("https://server.gamenode.app/x?y=1").unwrap()
        ));
        assert!(is_web_url(&Url::parse("http://localhost:5000/").unwrap()));
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "app.gamenode://home",
            "calc:",
            "ms-msdt:/id",
        ] {
            let url = Url::parse(bad).unwrap();
            assert!(!is_web_url(&url), "{bad}");
            assert!(SystemBrowser.open(&url).is_err());
        }
    }
}
