use super::*;
use crate::browser::SystemBrowser;
use gamenode_core::ports::BrowserOpener;

#[test]
fn web_and_local_file_schemes_are_never_handed_to_the_os() {
    for bad in [
        "https://example.com/",
        "http://example.com/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,hi",
        "vbscript:x",
    ] {
        let uri = Url::parse(bad).unwrap();
        assert!(!is_app_uri(&uri), "{bad}");
        assert_eq!(SystemUriOpener.open(&uri), Err(UriError::Failed), "{bad}");
    }
    assert!(is_app_uri(&Url::parse("steam://rungameid/620").unwrap()));
}

#[test]
fn missing_handlers_are_told_apart_from_other_failures() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        classify(&Error::from(ErrorKind::NotFound)),
        UriError::NoHandler
    );
    assert_eq!(
        classify(&Error::from_raw_os_error(1155)),
        UriError::NoHandler
    );
    assert_eq!(
        classify(&Error::from(ErrorKind::PermissionDenied)),
        UriError::Failed
    );
}

#[test]
fn the_browser_opener_still_accepts_only_web_urls() {
    for app in ["steam://rungameid/620", "file:///etc/passwd", "calc:"] {
        assert!(
            SystemBrowser.open(&Url::parse(app).unwrap()).is_err(),
            "{app}"
        );
    }
}
