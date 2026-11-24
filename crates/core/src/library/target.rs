//! Detected launch targets of library entries (task 7.7, design section 11).
//!
//! A provider may report a typed URI or a local executable for one of its own games. Reporting a
//! target only registers a candidate: it never starts anything. An executable target needs the
//! user's approval, and a changed target loses that approval, so an unreviewed scan can never run
//! code. The approval is bound to [`DetectedTarget::revision`]; a revalidation before every spawn
//! catches a replaced file or a folder that moved, and the entry then offers repair.
//!
//! Paths stay relative to the reported install folder, and the platform adapter canonicalizes
//! them and checks the file identity. Nothing here performs I/O.

use serde::{Deserialize, Serialize};

use crate::ports::is_safe_relative_path;

/// Longest accepted URI, in bytes.
pub const MAX_URI_BYTES: usize = 2048;
/// Longest accepted relative executable path, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Most arguments one target may carry.
pub const MAX_ARGUMENTS: usize = 64;
/// Longest accepted argument, in bytes.
pub const MAX_ARGUMENT_BYTES: usize = 4096;
/// Schemes a detected URI may never use: they open local content or script instead of a game.
const DENIED_SCHEMES: &[&str] = &[
    "about",
    "blob",
    "data",
    "file",
    "javascript",
    "ms-msdt",
    "search-ms",
    "shell",
    "vbscript",
];

/// What a provider reported for one game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TargetSpec {
    /// A URI the provider's own client handles.
    Uri { uri: String },
    /// An executable inside the game's install folder.
    Executable {
        /// The install folder the provider reported.
        root: String,
        /// Executable path relative to `root`.
        path: String,
        /// Literal arguments, in launch order.
        #[serde(default)]
        arguments: Vec<String>,
        /// Working directory relative to `root`; empty means `root` itself.
        #[serde(default)]
        working_directory: String,
    },
}

impl TargetSpec {
    /// Bounds and shape a provider's target. The host calls this before storing a candidate; a
    /// rejected target leaves the game's other facts intact.
    pub fn validate_shape(&self) -> bool {
        match self {
            Self::Uri { uri } => valid_uri(uri),
            Self::Executable {
                root,
                path,
                arguments,
                working_directory,
            } => {
                valid_root(root)
                    && path.len() <= MAX_PATH_BYTES
                    && is_safe_relative_path(path)
                    && (working_directory.is_empty()
                        || (working_directory.len() <= MAX_PATH_BYTES
                            && is_safe_relative_path(working_directory)))
                    && arguments.len() <= MAX_ARGUMENTS
                    && arguments.iter().all(|argument| {
                        argument.len() <= MAX_ARGUMENT_BYTES
                            && !argument.chars().any(char::is_control)
                    })
            }
        }
    }

    /// The URI, when this is a URI target.
    pub fn uri(&self) -> Option<&str> {
        match self {
            Self::Uri { uri } => Some(uri),
            Self::Executable { .. } => None,
        }
    }

    /// A stable list of the fields that make this target what it is. A changed list is a new
    /// target revision and loses the user's approval.
    pub fn fingerprint_parts(&self) -> Vec<String> {
        match self {
            Self::Uri { uri } => vec!["uri".into(), uri.clone()],
            Self::Executable {
                root,
                path,
                arguments,
                working_directory,
            } => {
                let mut parts = vec![
                    "executable".into(),
                    root.clone(),
                    path.clone(),
                    working_directory.clone(),
                ];
                parts.extend(arguments.iter().cloned());
                parts
            }
        }
    }
}

/// A stored target and the user's decision about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTarget {
    pub spec: TargetSpec,
    /// Last target the provider reported. A user repair may change `spec` while scans keep
    /// reporting this original value; a later provider change still creates a new revision.
    #[serde(default)]
    pub reported_spec: Option<TargetSpec>,
    /// `spec` was explicitly chosen by the user as a repair for the same provider candidate.
    #[serde(default)]
    pub user_override: bool,
    /// Grows when the provider reports a different target; approval is bound to it.
    pub revision: u64,
    /// Whether the user approved this revision.
    pub approved: bool,
    /// Whether the target was still reported and its executable still validated.
    pub available: bool,
    /// File identity the platform recorded at approval; `None` for URI targets.
    #[serde(default)]
    pub identity: Option<String>,
    /// Unix seconds of the scan that reported it.
    pub reported_at: i64,
}

impl DetectedTarget {
    /// The target after a scan reported `spec`. The same target keeps its revision and approval;
    /// a different one starts a new revision without approval, so changed paths ask again.
    pub fn reported(existing: Option<&Self>, spec: TargetSpec, now: i64) -> Self {
        if let Some(existing) = existing
            && existing.user_override
            && existing.reported_spec.as_ref() == Some(&spec)
        {
            return Self {
                available: true,
                reported_at: now,
                ..existing.clone()
            };
        }
        match existing {
            Some(existing) if existing.spec == spec => Self {
                available: true,
                reported_at: now,
                reported_spec: Some(spec),
                ..existing.clone()
            },
            _ => Self {
                revision: existing.map_or(1, |existing| existing.revision.saturating_add(1)),
                spec: spec.clone(),
                reported_spec: Some(spec),
                user_override: false,
                approved: false,
                available: true,
                identity: None,
                reported_at: now,
            },
        }
    }

    /// The same target, marked unavailable: a completed scan no longer reported it.
    pub fn missing(existing: &Self, now: i64) -> Self {
        Self {
            available: false,
            reported_at: now,
            ..existing.clone()
        }
    }

    /// User-selected replacement executable for a previously reported target.
    pub fn repaired(&self, spec: TargetSpec, identity: String, now: i64) -> Self {
        let changed = self.spec != spec;
        Self {
            revision: if changed {
                self.revision.saturating_add(1)
            } else {
                self.revision
            },
            spec,
            user_override: changed || self.user_override,
            approved: true,
            available: true,
            identity: Some(identity),
            reported_at: now,
            ..self.clone()
        }
    }

    /// A stable number for what a launch would start now. A remembered confirmation and a
    /// pre-issued intent are bound to it.
    ///
    /// This is a fingerprint, not a security boundary: approval and revision are checked
    /// separately before anything starts. It is computed without a crypto dependency so this
    /// crate stays free of I/O and platform crates.
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut part = |bytes: &[u8]| {
            for byte in (bytes.len() as u64).to_be_bytes().iter().chain(bytes) {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        part(b"detected");
        part(&self.revision.to_be_bytes());
        part(if self.approved {
            b"approved"
        } else {
            b"unapproved"
        });
        part(if self.available {
            b"available"
        } else {
            b"unavailable"
        });
        part(self.identity.as_deref().unwrap_or_default().as_bytes());
        for value in self.spec.fingerprint_parts() {
            part(value.as_bytes());
        }
        hash
    }

    /// Whether Play may run this target now.
    pub fn ready(&self) -> bool {
        self.approved && self.available
    }
}

/// Non-secret target state exposed in the selected library entry's detail view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTargetView {
    pub spec: TargetSpec,
    pub revision: u64,
    pub approved: bool,
    pub available: bool,
}

impl From<&DetectedTarget> for DetectedTargetView {
    fn from(target: &DetectedTarget) -> Self {
        Self {
            spec: target.spec.clone(),
            revision: target.revision,
            approved: target.approved,
            available: target.available,
        }
    }
}

fn valid_root(root: &str) -> bool {
    !root.trim().is_empty()
        && root.len() <= MAX_PATH_BYTES
        && !root.chars().any(char::is_control)
        && (root.starts_with('/')
            || root.starts_with('\\')
            || crate::provider::path::is_windows_style(root))
}

fn valid_uri(uri: &str) -> bool {
    if uri.is_empty()
        || uri.len() > MAX_URI_BYTES
        || uri.trim() != uri
        || uri.chars().any(char::is_control)
        || uri.chars().any(char::is_whitespace)
    {
        return false;
    }
    let Some((scheme, rest)) = uri.split_once(':') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    !DENIED_SCHEMES
        .iter()
        .any(|denied| scheme.eq_ignore_ascii_case(denied))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable() -> TargetSpec {
        TargetSpec::Executable {
            root: r"C:\Games\Celeste".into(),
            path: "Celeste.exe".into(),
            arguments: vec!["--fast".into()],
            working_directory: String::new(),
        }
    }

    #[test]
    fn a_new_target_starts_unapproved_and_a_same_one_keeps_its_approval() {
        let first = DetectedTarget::reported(None, executable(), 10);
        assert_eq!((first.revision, first.approved), (1, false));
        let approved = DetectedTarget {
            approved: true,
            identity: Some("vol:file".into()),
            ..first.clone()
        };
        let again = DetectedTarget::reported(Some(&approved), executable(), 20);
        assert_eq!((again.revision, again.approved), (1, true));
        assert_eq!(again.identity.as_deref(), Some("vol:file"));
        assert_eq!(again.reported_at, 20);
    }

    #[test]
    fn a_changed_target_gets_a_new_revision_and_loses_approval() {
        let approved = DetectedTarget {
            approved: true,
            identity: Some("vol:file".into()),
            ..DetectedTarget::reported(None, executable(), 10)
        };
        let changed = TargetSpec::Executable {
            root: r"C:\Games\Celeste".into(),
            path: "Celeste.exe".into(),
            arguments: vec!["--safe".into()],
            working_directory: String::new(),
        };
        let next = DetectedTarget::reported(Some(&approved), changed, 20);
        assert_eq!((next.revision, next.approved), (2, false));
        assert_eq!(next.identity, None);
    }

    #[test]
    fn an_unreported_target_stays_stored_but_unavailable() {
        let target = DetectedTarget {
            approved: true,
            ..DetectedTarget::reported(None, executable(), 10)
        };
        let missing = DetectedTarget::missing(&target, 30);
        assert!(!missing.available);
        assert!(missing.approved);
        assert_eq!(missing.revision, target.revision);
        assert!(!missing.ready());
    }

    #[test]
    fn a_user_repair_survives_the_same_scan_but_a_provider_change_revokes_it() {
        let reported = DetectedTarget::reported(None, executable(), 10);
        let replacement = TargetSpec::Executable {
            root: r"C:\Games\Celeste".into(),
            path: "bin/Celeste.exe".into(),
            arguments: vec!["--fast".into()],
            working_directory: String::new(),
        };
        let repaired = reported.repaired(replacement, "vol:new-file".into(), 20);
        assert!(repaired.ready());
        assert!(repaired.user_override);
        let repeated = DetectedTarget::reported(Some(&repaired), executable(), 30);
        assert_eq!(repeated.spec, repaired.spec);
        assert_eq!(repeated.revision, repaired.revision);
        assert!(repeated.ready());

        let changed = TargetSpec::Executable {
            root: r"C:\Games\Celeste".into(),
            path: "Celeste64.exe".into(),
            arguments: vec![],
            working_directory: String::new(),
        };
        let next = DetectedTarget::reported(Some(&repeated), changed, 40);
        assert_eq!(next.revision, repeated.revision + 1);
        assert!(!next.approved);
        assert!(!next.user_override);
        assert_eq!(next.identity, None);
    }

    #[test]
    fn target_view_does_not_expose_the_approved_file_identity() {
        let target = DetectedTarget {
            approved: true,
            identity: Some("volume:secret-file-id".into()),
            ..DetectedTarget::reported(None, executable(), 10)
        };
        let view = DetectedTargetView::from(&target);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("secret-file-id"));
        assert!(json.contains("revision"));
    }

    #[test]
    fn only_relative_paths_inside_the_reported_root_are_accepted() {
        for path in ["Celeste.exe", "bin/game.exe", r"bin\game.exe"] {
            let spec = TargetSpec::Executable {
                root: r"C:\Games\Celeste".into(),
                path: path.into(),
                arguments: vec![],
                working_directory: String::new(),
            };
            assert!(spec.validate_shape(), "{path}");
        }
        for path in [
            "",
            "..\\escape.exe",
            "/abs/game.exe",
            r"C:\abs.exe",
            "bin/../../x.exe",
        ] {
            let spec = TargetSpec::Executable {
                root: r"C:\Games\Celeste".into(),
                path: path.into(),
                arguments: vec![],
                working_directory: String::new(),
            };
            assert!(!spec.validate_shape(), "{path}");
        }
        let relative_root = TargetSpec::Executable {
            root: "Games/Celeste".into(),
            path: "Celeste.exe".into(),
            arguments: vec![],
            working_directory: String::new(),
        };
        assert!(!relative_root.validate_shape());
    }

    #[test]
    fn uris_keep_their_scheme_and_never_open_local_content() {
        for uri in [
            "steam://rungameid/620",
            "https://example.com/play?id=1",
            "uplay://launch/1/0",
        ] {
            assert!(
                TargetSpec::Uri { uri: uri.into() }.validate_shape(),
                "{uri}"
            );
        }
        for uri in [
            "",
            "steam:",
            " file:///C:/game.exe",
            "javascript:alert(1)",
            "data:text/html,x",
            "no scheme",
            "steam://run game",
        ] {
            assert!(
                !TargetSpec::Uri { uri: uri.into() }.validate_shape(),
                "{uri}"
            );
        }
    }

    #[test]
    fn arguments_are_bounded_and_never_shell_fragments() {
        let mut spec = executable();
        if let TargetSpec::Executable { arguments, .. } = &mut spec {
            arguments.push("bad\narg".into());
        }
        assert!(!spec.validate_shape());
    }
}
