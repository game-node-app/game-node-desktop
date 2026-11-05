//! Which GameNode image URLs the application downloads.
//!
//! GameNode reports covers and artworks as protocol-relative IGDB image URLs. Only those are
//! fetched: https, the IGDB image host, and the upload path with a size segment and a `.jpg` id.
//! The size is rewritten for the image's use in the library.

use url::Url;

/// The only host covers come from.
const HOST: &str = "images.igdb.com";
/// The size every download asks for.
const SIZE: &str = "t_cover_big";
/// The size used for the wide image in the details pane.
const ARTWORK_SIZE: &str = "t_1080p";

/// A cover URL that passed the check. Only [`download_url`] makes one, so a download can never be
/// started for a URL that was not checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverUrl(Url);

/// A landscape artwork URL that passed the same IGDB host and path checks as a cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkUrl(Url);

impl CoverUrl {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl ArtworkUrl {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

/// The URL to download a game's cover from, or `None` when the reported URL is not an allowed
/// IGDB image URL.
pub fn download_url(reported: &str) -> Option<CoverUrl> {
    image_url(reported, SIZE).map(CoverUrl)
}

/// The URL to download a game's artwork from, or `None` when it is not an allowed IGDB image URL.
pub fn artwork_download_url(reported: &str) -> Option<ArtworkUrl> {
    image_url(reported, ARTWORK_SIZE).map(ArtworkUrl)
}

fn image_url(reported: &str, target_size: &str) -> Option<Url> {
    let reported = reported.trim();
    let absolute = match reported.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => reported.to_owned(),
    };
    let url = Url::parse(&absolute).ok()?;

    let plain_host = url.scheme() == "https"
        && url.host_str() == Some(HOST)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    if !plain_host {
        return None;
    }

    // /igdb/image/upload/t_<size>/<id>.jpg
    let mut segments = url.path_segments()?;
    let (Some("igdb"), Some("image"), Some("upload"), Some(size), Some(file), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return None;
    };
    let size_name = size.strip_prefix("t_")?;
    let id = file.strip_suffix(".jpg")?;
    let token = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    if !token(size_name) || !token(id) {
        return None;
    }

    Url::parse(&format!(
        "https://{HOST}/igdb/image/upload/{target_size}/{id}.jpg"
    ))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewritten(reported: &str) -> Option<String> {
        download_url(reported).map(|cover| cover.as_url().to_string())
    }

    const BIG: &str = "https://images.igdb.com/igdb/image/upload/t_cover_big/co1wyy.jpg";

    #[test]
    fn the_servers_protocol_relative_url_is_made_https_and_the_size_rewritten() {
        assert_eq!(
            rewritten("//images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg").as_deref(),
            Some(BIG)
        );
    }

    #[test]
    fn an_https_url_of_any_size_is_allowed() {
        for size in [
            "t_thumb",
            "t_cover_small",
            "t_cover_big",
            "t_1080p",
            "t_720p",
        ] {
            let reported = format!("https://images.igdb.com/igdb/image/upload/{size}/co1wyy.jpg");
            assert_eq!(rewritten(&reported).as_deref(), Some(BIG), "{size}");
        }
        // Surrounding whitespace from a sloppy payload is tolerated.
        assert_eq!(
            rewritten(" //images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg\n").as_deref(),
            Some(BIG)
        );
    }

    #[test]
    fn plain_http_is_refused() {
        assert_eq!(
            rewritten("http://images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg"),
            None
        );
    }

    #[test]
    fn other_hosts_are_refused() {
        for reported in [
            "https://example.com/igdb/image/upload/t_thumb/co1wyy.jpg",
            "https://images.igdb.com.evil.test/igdb/image/upload/t_thumb/co1wyy.jpg",
            "https://evil.test/images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg",
            "https://images.igdb.com@evil.test/igdb/image/upload/t_thumb/co1wyy.jpg",
            "https://user:pw@images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg",
            "https://images.igdb.com:8443/igdb/image/upload/t_thumb/co1wyy.jpg",
            "//cdn.example.com/igdb/image/upload/t_thumb/co1wyy.jpg",
            "ftp://images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg",
        ] {
            assert_eq!(rewritten(reported), None, "{reported}");
        }
    }

    #[test]
    fn other_paths_are_refused() {
        for reported in [
            "https://images.igdb.com/",
            "https://images.igdb.com/igdb/image/upload/co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_thumb/co1wyy.png",
            "https://images.igdb.com/igdb/image/upload/t_thumb/co1wyy",
            "https://images.igdb.com/igdb/image/upload/t_thumb/.jpg",
            "https://images.igdb.com/igdb/image/upload/thumb/co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_/co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_thumb/a/co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_thumb/../co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_thumb/%2e%2e/co1wyy.jpg",
            "https://images.igdb.com/other/image/upload/t_thumb/co1wyy.jpg",
            "https://images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg?x=1",
            "https://images.igdb.com/igdb/image/upload/t_thumb/co1wyy.jpg#frag",
            "https://images.igdb.com/igdb/image/upload/t_thumb/co1%2Fyy.jpg",
        ] {
            assert_eq!(rewritten(reported), None, "{reported}");
        }
    }

    #[test]
    fn malformed_input_is_refused() {
        for reported in [
            "",
            "   ",
            "not a url",
            "//",
            "https://",
            "images.igdb.com/x.jpg",
        ] {
            assert_eq!(rewritten(reported), None, "{reported:?}");
        }
    }
}
