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
    if requested.is_empty() || requested.len() > ALL.len() + 1 {
        return Err(NegotiationError::Capability);
    }
    let mut seen = std::collections::BTreeSet::new();
    let selected = minor.min(crate::API_MINOR);
    for name in requested {
        if (!ALL.contains(&name.as_str()) && name != SETTINGS)
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
    }
    Ok(selected)
}
