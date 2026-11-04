//! Hands provider URIs (`steam://rungameid/620`) to the OS handler.

use gamenode_core::ports::{UriError, UriOpener};
use url::Url;

/// The OS handler for a URI scheme (`ShellExecute` on Windows, `xdg-open` on Linux). The URI is
/// passed as one argument, never through a shell.
///
/// Callers only pass URIs built from a provider's own template, which keeps the template's
/// scheme. As a backstop, schemes that mean "a web page" or "a local file or script" are refused
/// here: web pages go through [`crate::browser::SystemBrowser`], and nothing here starts files.
pub struct SystemUriOpener;

impl UriOpener for SystemUriOpener {
    fn open(&self, uri: &Url) -> Result<(), UriError> {
        if !is_app_uri(uri) {
            return Err(UriError::Failed);
        }
        // Waits until the OS (or `xdg-open`) has accepted the URI, so a missing handler is
        // reported instead of silently ignored.
        open::that(uri.as_str()).map_err(|error| {
            tracing::warn!(kind = ?error.kind(), scheme = uri.scheme(), "could not open a provider link");
            classify(&error)
        })
    }
}

/// `ERROR_NO_ASSOCIATION`: Windows has no handler for the scheme.
const WINDOWS_NO_ASSOCIATION: i32 = 1155;

fn classify(error: &std::io::Error) -> UriError {
    if error.kind() == std::io::ErrorKind::NotFound
        || error.raw_os_error() == Some(WINDOWS_NO_ASSOCIATION)
    {
        UriError::NoHandler
    } else {
        UriError::Failed
    }
}

fn is_app_uri(uri: &Url) -> bool {
    !matches!(
        uri.scheme(),
        "http" | "https" | "file" | "javascript" | "data" | "vbscript" | "blob"
    )
}

#[cfg(test)]
mod tests;
