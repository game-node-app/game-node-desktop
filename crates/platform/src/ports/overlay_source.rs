//! The GameNode port overlay: bundled, stored and refreshed from the repository (design D4).
//!
//! `ports/overlay.toml` in this repository is embedded at build time with [`include_str!`], so PC
//! ports always has a valid overlay, even on the first run or offline. The host may fetch a newer
//! copy from [`SOURCE`] with an ETag; a copy that fails to parse or validate, or that declares an
//! unknown major `schema_version`, is ignored whole and the newest valid copy keeps being used.
//!
//! The stored copy lives in [`PortStore`], beside the catalog snapshot, so a restart reopens the
//! last overlay that was accepted. Fetching is blocking and belongs on a worker thread; the
//! catalog worker decides when to call [`OverlaySource::refresh`].

use std::io::Read;
use std::sync::Arc;

use gamenode_core::ports::StorageError;
use gamenode_core::ports::overlay::{Overlay, SUPPORTED_SCHEMA_VERSION};
use gamenode_core::ports::store::{PortOverlayRow, PortStore};

/// Where newer overlay documents are published.
///
/// The repository is public and the URL is fixed, so the host never fetches an arbitrary address.
pub const SOURCE: &str =
    "https://raw.githubusercontent.com/game-node-app/game-node-desktop/main/ports/overlay.toml";

/// The overlay authored in this repository and embedded at build time.
pub const BUNDLED: &str = include_str!("../../../../ports/overlay.toml");

/// The largest overlay document accepted from the network.
///
/// The real overlay is a few kilobytes; the cap only keeps a wrong URL from filling memory.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// One answer from the overlay source.
///
/// Network and HTTP failures are ordinary outcomes: the previous overlay stays in use, so they
/// never fail the refresh. Only a storage failure is an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchedOverlay {
    /// HTTP 304: the copy behind the stored ETag is still current.
    NotModified,
    /// HTTP 200 with the document and the ETag it can be checked with later.
    Modified {
        etag: Option<String>,
        payload: String,
    },
    /// The request failed, or the answer cannot be used (status, size, encoding).
    Failed { reason: String },
}

/// Fetches the overlay document. Blocking: run it on a worker thread.
pub trait OverlayFetcher: Send + Sync {
    /// `etag` is the stored copy's ETag, when there is one. Implementations send it as
    /// `If-None-Match` so an unchanged document costs a 304.
    fn fetch(&self, etag: Option<&str>) -> FetchedOverlay;
}

/// The real fetcher, using the same blocking stack and limits as the manifest cache.
pub struct HttpOverlayFetcher {
    source: String,
}

impl HttpOverlayFetcher {
    pub fn new() -> Self {
        Self::from_url(SOURCE)
    }

    /// A fetcher for a different address, used by tests and by offline development.
    pub fn from_url(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

impl Default for HttpOverlayFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayFetcher for HttpOverlayFetcher {
    fn fetch(&self, etag: Option<&str>) -> FetchedOverlay {
        let client = match reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
        {
            Ok(client) => client,
            Err(_) => {
                return FetchedOverlay::Failed {
                    reason: "the HTTP client could not be created".into(),
                };
            }
        };
        let mut request = client.get(&self.source);
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let response = match request.send() {
            Ok(response) => response,
            Err(_) => {
                return FetchedOverlay::Failed {
                    reason: "the overlay source is unreachable".into(),
                };
            }
        };
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return FetchedOverlay::NotModified;
        }
        if response.status() != reqwest::StatusCode::OK
            || response
                .content_length()
                .is_some_and(|length| length > MAX_BYTES)
        {
            return FetchedOverlay::Failed {
                reason: format!("the overlay source answered {}", response.status().as_u16()),
            };
        }
        // An unusable ETag costs the next conditional request, not this document.
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        if response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return FetchedOverlay::Failed {
                reason: "the overlay document could not be read".into(),
            };
        }
        if bytes.len() as u64 > MAX_BYTES {
            return FetchedOverlay::Failed {
                reason: "the overlay document is too large".into(),
            };
        }
        match String::from_utf8(bytes) {
            Ok(payload) => FetchedOverlay::Modified { etag, payload },
            Err(_) => FetchedOverlay::Failed {
                reason: "the overlay document is not UTF-8".into(),
            },
        }
    }
}

/// The overlay in use and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveOverlay {
    pub overlay: Overlay,
    /// The ETag of the stored copy, so the next refresh can ask for a 304. `None` for the bundled
    /// copy.
    pub etag: Option<String>,
    /// True when no valid copy is stored and the bundled one is in use.
    pub bundled: bool,
}

/// What a refresh did. Every failure leaves the previous overlay in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// A newer, valid overlay replaced the stored one. The caller passes it to the catalog worker
    /// (see `CatalogWorker::set_overlay`) so the list re-merges without reading the database again.
    Replaced { overlay: Overlay },
    /// The source answered 304; the stored copy stays.
    NotModified,
    /// The request failed, the answer was unusable or the document did not validate. The previous
    /// copy (stored or bundled) stays in use.
    KeptPrevious { reason: String },
}

/// The bundled overlay plus the newest valid stored copy.
pub struct OverlaySource {
    store: Arc<dyn PortStore>,
}

impl OverlaySource {
    pub fn new(store: Arc<dyn PortStore>) -> Self {
        Self { store }
    }

    /// The overlay to merge into the catalog right now.
    ///
    /// A stored copy that no longer parses, or that declares an unknown major version, is ignored
    /// in favour of the bundled copy. The only possible error is an invalid bundled overlay, which
    /// would be a build error.
    pub fn current(&self) -> Result<ActiveOverlay, StorageError> {
        let row = self.store.overlay()?;
        if let Some(row) = row {
            // Reject an incompatible row before parsing it, as the migration records it for that.
            if row.version <= SUPPORTED_SCHEMA_VERSION {
                match Overlay::parse(&row.payload) {
                    Ok(overlay) => {
                        return Ok(ActiveOverlay {
                            overlay,
                            etag: row.etag,
                            bundled: false,
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "ignored an invalid stored port overlay");
                    }
                }
            } else {
                tracing::warn!(
                    version = row.version,
                    "ignored a stored port overlay from a newer schema"
                );
            }
        }
        let overlay = Overlay::parse(BUNDLED).map_err(|error| {
            StorageError::Failed(format!("the bundled port overlay is invalid: {error}"))
        })?;
        Ok(ActiveOverlay {
            overlay,
            etag: None,
            bundled: true,
        })
    }

    /// Fetches a newer copy and stores it only when it parses and validates.
    ///
    /// The previous overlay is kept on a network error, a 304 without a stored copy, a response
    /// that is not 200, a document that fails to parse, and a document that declares an unknown
    /// major version. The caller decides how often to refresh; the ETag keeps the request cheap.
    pub fn refresh(&self, fetcher: &dyn OverlayFetcher) -> Result<RefreshOutcome, StorageError> {
        let stored = self.store.overlay()?;
        let usable = stored.filter(|row| {
            row.version <= SUPPORTED_SCHEMA_VERSION && Overlay::parse(&row.payload).is_ok()
        });
        let etag = usable.as_ref().and_then(|row| row.etag.clone());

        match fetcher.fetch(etag.as_deref()) {
            FetchedOverlay::NotModified => {
                if etag.is_some() {
                    Ok(RefreshOutcome::NotModified)
                } else {
                    // Nothing was stored to be unchanged: treat the answer as unusable rather
                    // than pretend a copy was confirmed.
                    Ok(RefreshOutcome::KeptPrevious {
                        reason: "the overlay source answered 304 without a stored overlay".into(),
                    })
                }
            }
            FetchedOverlay::Failed { reason } => Ok(RefreshOutcome::KeptPrevious { reason }),
            FetchedOverlay::Modified { etag, payload } => {
                let overlay = match Overlay::parse(&payload) {
                    Ok(overlay) => overlay,
                    Err(error) => {
                        tracing::warn!(%error, "ignored an invalid fetched port overlay");
                        return Ok(RefreshOutcome::KeptPrevious {
                            reason: error.to_string(),
                        });
                    }
                };
                let row = PortOverlayRow {
                    version: overlay.schema_version,
                    etag,
                    payload,
                };
                self.store.save_overlay(&row)?;
                Ok(RefreshOutcome::Replaced { overlay })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Records the ETag each request was conditioned on and answers with one canned outcome.
    struct FakeFetcher {
        outcome: FetchedOverlay,
        seen: Mutex<Vec<Option<String>>>,
    }

    impl FakeFetcher {
        fn new(outcome: FetchedOverlay) -> Self {
            Self {
                outcome,
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<Option<String>> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl OverlayFetcher for FakeFetcher {
        fn fetch(&self, etag: Option<&str>) -> FetchedOverlay {
            self.seen.lock().unwrap().push(etag.map(str::to_owned));
            self.outcome.clone()
        }
    }

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("ports.sqlite3")
    }

    fn open_store(path: &std::path::Path) -> crate::ports_db::SqlitePortStore {
        crate::db::SqliteSettingsStore::open(path)
            .unwrap()
            .ports_store()
    }

    fn source(store: &Arc<crate::ports_db::SqlitePortStore>) -> OverlaySource {
        let concrete: Arc<crate::ports_db::SqlitePortStore> = Arc::clone(store);
        OverlaySource::new(concrete)
    }

    fn valid_payload(edition: &str) -> String {
        format!("schema_version = 1\n\n[entry.\"qcat_x\"]\nedition = \"{edition}\"\n")
    }

    fn stored_overlay(edition: &str, etag: Option<&str>) -> PortOverlayRow {
        PortOverlayRow {
            version: 1,
            etag: etag.map(str::to_owned),
            payload: valid_payload(edition),
        }
    }

    #[test]
    fn the_bundled_overlay_pins_the_ps4_extractor_and_leaves_the_version_to_lantern_engine() {
        let overlay = Overlay::parse(BUNDLED).expect("the bundled overlay must parse");
        assert_eq!(overlay.schema_version, SUPPORTED_SCHEMA_VERSION);
        assert_eq!(overlay.extractor.len(), 1);
        let extractor = &overlay.extractor[0];
        assert_eq!(extractor.id, "ps4-pkg");
        assert_eq!(extractor.name, "ps4-pkg-extractor");
        assert_eq!(extractor.repository, "paulomanrique/ps4-pkg-extractor");
        assert_eq!(extractor.tag, "v0.0.1");
        assert_eq!(extractor.asset, "pkgextract-x86_64-pc-windows-msvc.zip");
        assert_eq!(
            extractor.sha256,
            "0c736ee02779fb5cf4403a91da23f94fdaa72debf995be0c400ac9c36acee63a"
        );
        assert_eq!(extractor.license, "Unlicense");
        assert_eq!(extractor.executable, "pkgextract.exe");
        assert_eq!(
            extractor.args,
            ["{package}", "-o", "{out}", "--overwrite"],
            "the tool refuses an existing output folder without --overwrite"
        );
        assert!(
            !extractor.args.iter().any(|argument| argument == "{update}"),
            "this tool takes one package per run; GameNode merges the update itself"
        );

        let lantern = overlay
            .entry
            .get("qcat_66376ffd-c54c-43de-aeab-d8da689a422b")
            .expect("the Lantern Engine entry is authored");
        let slots = lantern
            .setup_file
            .as_ref()
            .expect("the Lantern Engine entry asks for the game folder");
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].contains, ["eboot.bin", "sce_sys/param.sfo"]);
        assert_eq!(slots[0].destination.as_deref(), Some("CUSA03173"));
        assert!(slots[0].accepted.is_empty(), "the port checks the version");
        assert_eq!(slots[0].hash_file, None);
        let package = slots[0]
            .from_package
            .as_ref()
            .expect("the slot offers the package option");
        assert_eq!(package.extractor, "ps4-pkg");
        assert_eq!(package.needs_update.as_deref(), Some("01.09"));
        assert_eq!(
            package.title_id, None,
            "Lantern Engine accepts several retail title ids"
        );

        // Merging the overlay pins the extractor on the entry the install worker receives.
        let merged = overlay
            .merge(gamenode_core::ports::CatalogEntry {
                catalog_id: "qcat_66376ffd-c54c-43de-aeab-d8da689a422b".into(),
                supported_os: vec!["windows".into()],
                ..Default::default()
            })
            .expect("the Lantern Engine entry is visible");
        assert_eq!(merged.extractors.len(), 1);
        assert_eq!(merged.extractors[0].id, "ps4-pkg");
    }

    #[test]
    fn current_uses_the_bundled_copy_until_a_valid_one_is_stored() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let source = source(&store);

        let active = source.current().unwrap();
        assert!(active.bundled);
        assert_eq!(active.etag, None);

        store
            .save_overlay(&stored_overlay("Director's Cut", Some("\"one\"")))
            .unwrap();
        let active = source.current().unwrap();
        assert!(!active.bundled);
        assert_eq!(active.etag.as_deref(), Some("\"one\""));
        assert!(active.overlay.entry.contains_key("qcat_x"));
    }

    #[test]
    fn an_invalid_fetched_overlay_keeps_the_previous_valid_one() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let previous = stored_overlay("Previous", Some("\"one\""));
        store.save_overlay(&previous).unwrap();

        let fetcher = FakeFetcher::new(FetchedOverlay::Modified {
            etag: Some("\"two\"".into()),
            payload: "schema_version = \n".into(),
        });
        assert!(matches!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::KeptPrevious { .. }
        ));
        assert_eq!(store.overlay().unwrap(), Some(previous));
        assert_eq!(fetcher.seen(), vec![Some("\"one\"".to_string())]);
    }

    #[test]
    fn an_unknown_major_or_an_invalid_stored_overlay_keeps_the_previous_one() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let previous = stored_overlay("Previous", Some("\"one\""));
        store.save_overlay(&previous).unwrap();

        let newer = FetchedOverlay::Modified {
            etag: Some("\"two\"".into()),
            payload: valid_payload("Newer").replace("schema_version = 1", "schema_version = 2"),
        };
        assert!(matches!(
            source(&store).refresh(&FakeFetcher::new(newer)).unwrap(),
            RefreshOutcome::KeptPrevious { .. }
        ));
        assert_eq!(store.overlay().unwrap(), Some(previous));

        // A stored row from a newer schema does not replace the bundled copy either.
        store
            .save_overlay(&PortOverlayRow {
                version: 9,
                etag: None,
                payload: valid_payload("Newer"),
            })
            .unwrap();
        let active = source(&store).current().unwrap();
        assert!(active.bundled, "an unknown major version is not used");
    }

    #[test]
    fn a_304_keeps_the_stored_overlay() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let previous = stored_overlay("Previous", Some("\"one\""));
        store.save_overlay(&previous).unwrap();

        let fetcher = FakeFetcher::new(FetchedOverlay::NotModified);
        assert_eq!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::NotModified
        );
        assert_eq!(store.overlay().unwrap(), Some(previous));
        assert_eq!(fetcher.seen(), vec![Some("\"one\"".to_string())]);
    }

    #[test]
    fn a_304_without_a_stored_overlay_keeps_the_bundled_one() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let fetcher = FakeFetcher::new(FetchedOverlay::NotModified);

        assert!(matches!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::KeptPrevious { .. }
        ));
        assert_eq!(store.overlay().unwrap(), None);
        assert!(source(&store).current().unwrap().bundled);
        assert_eq!(fetcher.seen(), vec![None]);
    }

    #[test]
    fn a_newer_valid_overlay_replaces_and_persists_with_its_etag() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        store
            .save_overlay(&stored_overlay("Previous", Some("\"one\"")))
            .unwrap();

        let payload = valid_payload("Newer");
        let fetcher = FakeFetcher::new(FetchedOverlay::Modified {
            etag: Some("\"two\"".into()),
            payload: payload.clone(),
        });
        // The refreshed overlay comes back ready to hand to the catalog worker.
        assert_eq!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::Replaced {
                overlay: Overlay::parse(&payload).unwrap()
            }
        );
        assert_eq!(
            store.overlay().unwrap(),
            Some(PortOverlayRow {
                version: 1,
                etag: Some("\"two\"".into()),
                payload: payload.clone(),
            })
        );

        let active = source(&store).current().unwrap();
        assert!(!active.bundled);
        assert_eq!(active.etag.as_deref(), Some("\"two\""));
        assert_eq!(
            active.overlay.entry["qcat_x"].edition.as_deref(),
            Some("Newer")
        );

        // The stored copy and its ETag survive a restart.
        drop(store);
        let reopened = Arc::new(open_store(&path));
        assert_eq!(
            reopened.overlay().unwrap().map(|row| row.etag),
            Some(Some("\"two\"".into()))
        );
        let fetcher = FakeFetcher::new(FetchedOverlay::NotModified);
        assert_eq!(
            source(&reopened).refresh(&fetcher).unwrap(),
            RefreshOutcome::NotModified
        );
        assert_eq!(fetcher.seen(), vec![Some("\"two\"".to_string())]);
    }

    #[test]
    fn an_unusable_etag_still_stores_the_valid_document() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let payload = valid_payload("Newer");
        let fetcher = FakeFetcher::new(FetchedOverlay::Modified {
            etag: None,
            payload: payload.clone(),
        });

        assert!(matches!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::Replaced { .. }
        ));
        assert_eq!(store.overlay().unwrap().map(|row| row.etag), Some(None));

        // Without an ETag the next request is unconditional instead of conditional on junk.
        let fetcher = FakeFetcher::new(FetchedOverlay::NotModified);
        assert!(matches!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::KeptPrevious { .. }
        ));
        assert_eq!(fetcher.seen(), vec![None]);
        assert_eq!(store.overlay().unwrap().unwrap().payload, payload);
    }

    #[test]
    fn a_network_error_keeps_the_previous_overlay() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = Arc::new(open_store(&path));
        let previous = stored_overlay("Previous", Some("\"one\""));
        store.save_overlay(&previous).unwrap();

        let fetcher = FakeFetcher::new(FetchedOverlay::Failed {
            reason: "the overlay source is unreachable".into(),
        });
        assert_eq!(
            source(&store).refresh(&fetcher).unwrap(),
            RefreshOutcome::KeptPrevious {
                reason: "the overlay source is unreachable".into()
            }
        );
        assert_eq!(store.overlay().unwrap(), Some(previous));
    }
}
