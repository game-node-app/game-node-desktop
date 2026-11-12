//! Public permission identifiers and protocol compatibility rules.

pub const CONTEXT: &str = "provider.context.read";
pub const DISCOVERY: &str = "library.discovery";
pub const CATALOG: &str = "library.catalog";
pub const ACCOUNT_READ: &str = "account.state.read";
pub const ACCOUNT_PUBLISH: &str = "account.state.publish";
pub const LOGIN: &str = "auth.webview";
pub const COOKIES: &str = "auth.webview.cookies.read";
pub const PAGE: &str = "auth.webview.page.read";
pub const SECRET_READ: &str = "secrets.read";
pub const SECRET_WRITE: &str = "secrets.write";
pub const SECRET_DELETE: &str = "secrets.delete";
pub const COUNTER: &str = "playtime.counter";
pub const ACTIONS: &str = "provider.actions.declare";
/// Optional feature. Deliberately excluded from ALL so existing bundled grants
/// cannot silently gain configuration authority when the protocol is updated.
pub const SETTINGS: &str = "ui.settings";
pub const SETTINGS_API_MINOR: u32 = 3;
pub const CONFIGURE: &str = "ui.configure";
pub const LIBRARY_READ: &str = "library.read";
pub const LIBRARY_READ_ALL: &str = "library.read.all";
pub const LIBRARY_METADATA_WRITE: &str = "library.metadata.write";
pub const FAVORITES_READ: &str = "favorites.read";
pub const FAVORITES_WRITE: &str = "favorites.write";
pub const MATCHING_READ: &str = "matching.read";
pub const MATCHING_WRITE: &str = "matching.write";
pub const ARTWORK_READ: &str = "artwork.read";
pub const ARTWORK_WRITE: &str = "artwork.write";
pub const GAME_LAUNCH: &str = "game.launch";
pub const GAME_CUSTOM_LAUNCH: &str = "game.custom_launch";
pub const GAMES_INSTALL: &str = "games.install";
pub const STORAGE_READ: &str = "storage.read";
pub const STORAGE_WRITE: &str = "storage.write";
pub const STORAGE_DELETE: &str = "storage.delete";
pub const STORAGE_PERSIST: &str = "storage.persist";
pub const FILES_READ: &str = "files.read";
pub const FILES_WRITE: &str = "files.write";
pub const FILES_DELETE: &str = "files.delete";
pub const NETWORK_FETCH: &str = "network.fetch";
pub const NOTIFICATIONS_PUBLISH: &str = "notifications.publish";
pub const BACKUPS_READ: &str = "backups.read";
pub const BACKUPS_CREATE: &str = "backups.create";
pub const BACKUPS_RESTORE: &str = "backups.restore";
pub const LIBRARY_ROOTS_READ: &str = "library.roots.read";
pub const LIBRARY_ROOTS_CREATE: &str = "library.roots.create";
pub const LIBRARY_ROOTS_UPDATE: &str = "library.roots.update";
pub const LIBRARY_ROOTS_DELETE: &str = "library.roots.delete";
pub const GAME_RUNTIME_READ: &str = "game.runtime.read";
pub const GAME_RUNTIME_CREATE: &str = "game.runtime.create";
pub const GAME_RUNTIME_UPDATE: &str = "game.runtime.update";
pub const GAME_RUNTIME_DELETE: &str = "game.runtime.delete";
pub const GAME_SAVES_READ: &str = "game.saves.read";
pub const GAME_SAVES_CREATE: &str = "game.saves.create";
pub const GAME_SAVES_UPDATE: &str = "game.saves.update";
pub const GAME_SAVES_DELETE: &str = "game.saves.delete";
pub const STORAGE_DIAGNOSTICS_PUBLISH: &str = "storage.diagnostics.publish";
pub const COMPUTER_HARDWARE_PUBLISH: &str = "computer.hardware.publish";
pub const GAME_PRESENCE_PUBLISH: &str = "game.presence.publish";
pub const GAME_PERFORMANCE_PUBLISH: &str = "game.performance.publish";
pub const GAME_SESSIONS_REPORT: &str = "game.sessions.report";
pub const DATA_PUBLISH: &str = "data.publish";
pub const TOOLS_READ: &str = "tools.read";
pub const TOOLS_APPLY: &str = "tools.apply";
pub const TOOLS_REMOVE: &str = "tools.remove";
pub const PORTS_READ: &str = "ports.read";
pub const PORTS_INSTALL: &str = "ports.install";
pub const THIRD_PARTY_API_MINOR: u32 = 4;

/// Public API feature descriptors used by API 1.4 range negotiation.
pub const FEATURE_API_METADATA: &str = "api.metadata";
pub const FEATURE_PERMISSION_WATCH: &str = "permissions.watch";
pub const FEATURE_EXTENSION_SESSION: &str = "session.extension";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RangeNegotiationError {
    #[error("no mutually supported API version")]
    Version,
    #[error("required feature is unavailable")]
    RequiredFeature,
    #[error("feature request is invalid")]
    InvalidFeature,
    #[error("session kind or target context is invalid")]
    Session,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureSelection {
    pub api_major: u32,
    pub api_minor: u32,
    pub features: Vec<(String, bool, u32)>,
}

/// Select the highest overlap. An empty range denotes a legacy 1.x client
/// whose `api_major/api_minor` fields retain their original meaning.
pub fn negotiate_range(
    legacy_major: u32,
    legacy_minor: u32,
    min_major: u32,
    min_minor: u32,
    max_major: u32,
    max_minor: u32,
    requested: &[(String, bool)],
) -> Result<FeatureSelection, RangeNegotiationError> {
    let legacy = min_major == 0 && min_minor == 0 && max_major == 0 && max_minor == 0;
    let (lo_major, lo_minor, hi_major, hi_minor) = if legacy {
        (
            legacy_major,
            crate::MIN_API_MINOR,
            legacy_major,
            legacy_minor,
        )
    } else {
        (min_major, min_minor, max_major, max_minor)
    };
    if lo_major > crate::API_MAJOR || hi_major < crate::API_MAJOR || lo_major > hi_major {
        return Err(RangeNegotiationError::Version);
    }
    let selected = hi_minor.min(crate::API_MINOR);
    let floor = if lo_major == crate::API_MAJOR {
        lo_minor
    } else {
        0
    };
    if selected < floor || selected < crate::MIN_API_MINOR {
        return Err(RangeNegotiationError::Version);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut features = Vec::with_capacity(requested.len());
    for (id, required) in requested {
        if id.is_empty() || !seen.insert(id) {
            return Err(RangeNegotiationError::InvalidFeature);
        }
        let introduced = match id.as_str() {
            FEATURE_API_METADATA
            | FEATURE_PERMISSION_WATCH
            | FEATURE_EXTENSION_SESSION
            | CONFIGURE => THIRD_PARTY_API_MINOR,
            SETTINGS => SETTINGS_API_MINOR,
            _ => 0,
        };
        let supported = introduced != 0 && introduced <= selected;
        if *required && !supported {
            return Err(RangeNegotiationError::RequiredFeature);
        }
        features.push((id.clone(), supported, introduced));
    }
    Ok(FeatureSelection {
        api_major: crate::API_MAJOR,
        api_minor: selected,
        features,
    })
}

pub fn validate_session_scope(kind: u32, has_target: bool) -> Result<(), RangeNegotiationError> {
    match kind {
        0 | 1 if !has_target => Ok(()),
        2 if has_target => Ok(()),
        _ => Err(RangeNegotiationError::Session),
    }
}

pub fn validate_target(
    target: &crate::proto::TargetGameContext,
) -> Result<(), RangeNegotiationError> {
    let valid = |value: &str, max: usize| {
        !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
    };
    if valid(&target.game_id, 256)
        && (target.installation_id.is_empty() || valid(&target.installation_id, 256))
        && (target.profile_id.is_empty() || valid(&target.profile_id, 256))
        && (target.revision.is_empty() || valid(&target.revision, 256))
    {
        Ok(())
    } else {
        Err(RangeNegotiationError::Session)
    }
}

pub const ALL: &[&str] = &[
    CONTEXT,
    DISCOVERY,
    CATALOG,
    ACCOUNT_READ,
    ACCOUNT_PUBLISH,
    LOGIN,
    COOKIES,
    PAGE,
    SECRET_READ,
    SECRET_WRITE,
    SECRET_DELETE,
    COUNTER,
    ACTIONS,
];

/// All identifiers understood by this API. Unlike [`ALL`], this list is not a
/// default grant set and must never be used to authorize an official plugin.
pub const KNOWN: &[&str] = &[
    CONTEXT,
    DISCOVERY,
    CATALOG,
    ACCOUNT_READ,
    ACCOUNT_PUBLISH,
    LOGIN,
    COOKIES,
    PAGE,
    SECRET_READ,
    SECRET_WRITE,
    SECRET_DELETE,
    COUNTER,
    ACTIONS,
    SETTINGS,
    CONFIGURE,
    LIBRARY_READ,
    LIBRARY_READ_ALL,
    LIBRARY_METADATA_WRITE,
    FAVORITES_READ,
    FAVORITES_WRITE,
    MATCHING_READ,
    MATCHING_WRITE,
    ARTWORK_READ,
    ARTWORK_WRITE,
    GAME_LAUNCH,
    GAME_CUSTOM_LAUNCH,
    GAMES_INSTALL,
    STORAGE_READ,
    STORAGE_WRITE,
    STORAGE_DELETE,
    STORAGE_PERSIST,
    FILES_READ,
    FILES_WRITE,
    FILES_DELETE,
    NETWORK_FETCH,
    NOTIFICATIONS_PUBLISH,
    BACKUPS_READ,
    BACKUPS_CREATE,
    BACKUPS_RESTORE,
    LIBRARY_ROOTS_READ,
    LIBRARY_ROOTS_CREATE,
    LIBRARY_ROOTS_UPDATE,
    LIBRARY_ROOTS_DELETE,
    GAME_RUNTIME_READ,
    GAME_RUNTIME_CREATE,
    GAME_RUNTIME_UPDATE,
    GAME_RUNTIME_DELETE,
    GAME_SAVES_READ,
    GAME_SAVES_CREATE,
    GAME_SAVES_UPDATE,
    GAME_SAVES_DELETE,
    STORAGE_DIAGNOSTICS_PUBLISH,
    COMPUTER_HARDWARE_PUBLISH,
    GAME_PRESENCE_PUBLISH,
    GAME_PERFORMANCE_PUBLISH,
    GAME_SESSIONS_REPORT,
    DATA_PUBLISH,
    TOOLS_READ,
    TOOLS_APPLY,
    TOOLS_REMOVE,
    PORTS_READ,
    PORTS_INSTALL,
];
pub const API_14_CAPABILITIES: &[&str] = &[
    CONFIGURE,
    LIBRARY_READ,
    LIBRARY_READ_ALL,
    LIBRARY_METADATA_WRITE,
    FAVORITES_READ,
    FAVORITES_WRITE,
    MATCHING_READ,
    MATCHING_WRITE,
    ARTWORK_READ,
    ARTWORK_WRITE,
    GAME_LAUNCH,
    GAME_CUSTOM_LAUNCH,
    GAMES_INSTALL,
    STORAGE_READ,
    STORAGE_WRITE,
    STORAGE_DELETE,
    STORAGE_PERSIST,
    FILES_READ,
    FILES_WRITE,
    FILES_DELETE,
    NETWORK_FETCH,
    NOTIFICATIONS_PUBLISH,
    BACKUPS_READ,
    BACKUPS_CREATE,
    BACKUPS_RESTORE,
    LIBRARY_ROOTS_READ,
    LIBRARY_ROOTS_DELETE,
    GAME_RUNTIME_READ,
    GAME_RUNTIME_DELETE,
    STORAGE_DIAGNOSTICS_PUBLISH,
    COMPUTER_HARDWARE_PUBLISH,
    GAME_PRESENCE_PUBLISH,
    GAME_PERFORMANCE_PUBLISH,
    GAME_SESSIONS_REPORT,
    DATA_PUBLISH,
    TOOLS_READ,
    TOOLS_APPLY,
    TOOLS_REMOVE,
    PORTS_READ,
    PORTS_INSTALL,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NegotiationError {
    #[error("incompatible Plugin API")]
    Version,
    #[error("plugin capability is not authorized")]
    Capability,
    #[error("required plugin capability is not supported by this API version")]
    Unsupported,
}

/// The request's minor is its newest supported minor within the declared major.
/// Unknown capabilities are denied even if a descriptor accidentally contains one.
pub fn negotiate(
    major: u32,
    minor: u32,
    requested: &[String],
    authorized: &[&str],
) -> Result<u32, NegotiationError> {
    if major != crate::API_MAJOR || minor < crate::MIN_API_MINOR {
        return Err(NegotiationError::Version);
    }
    if requested.is_empty() || requested.len() > KNOWN.len() {
        return Err(NegotiationError::Capability);
    }
    let mut seen = std::collections::BTreeSet::new();
    let selected = minor.min(crate::API_MINOR);
    for name in requested {
        if !KNOWN.contains(&name.as_str())
            || !authorized.contains(&name.as_str())
            || !seen.insert(name)
        {
            return Err(NegotiationError::Capability);
        }
        if name != DISCOVERY && selected < 2 {
            return Err(NegotiationError::Unsupported);
        }
        if name == SETTINGS && selected < SETTINGS_API_MINOR {
            return Err(NegotiationError::Unsupported);
        }
        if name == CONFIGURE && selected < THIRD_PARTY_API_MINOR {
            return Err(NegotiationError::Unsupported);
        }
        if API_14_CAPABILITIES.contains(&name.as_str()) && selected < THIRD_PARTY_API_MINOR {
            return Err(NegotiationError::Unsupported);
        }
    }
    Ok(selected)
}
