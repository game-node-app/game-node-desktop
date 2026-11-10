//! Official Epic Games Store integration. Production dependencies use only the
//! public plugin protocol/SDK and independent libraries; no GameNode internal
//! crates and no host process code.
pub mod account;
pub mod bridge;
pub mod discovery;
pub mod http;
pub mod runtime;

/// The trusted plugin identity declared by `plugins/epic.toml`.
pub const PLUGIN_ID: &str = "org.gamenode.plugin.epic";
/// The store provider id declared by `providers/epic.toml`.
pub const PROVIDER_ID: &str = "epic";

/// The descriptor grants exactly these capabilities. Epic uses a cookie-free
/// login and never reads achievements.
pub const CAPABILITIES: &[&str] = &[
    gamenode_plugin_protocol::capability::CONTEXT,
    gamenode_plugin_protocol::capability::DISCOVERY,
    gamenode_plugin_protocol::capability::CATALOG,
    gamenode_plugin_protocol::capability::ACCOUNT_READ,
    gamenode_plugin_protocol::capability::ACCOUNT_PUBLISH,
    gamenode_plugin_protocol::capability::LOGIN,
    gamenode_plugin_protocol::capability::PAGE,
    gamenode_plugin_protocol::capability::SECRET_READ,
    gamenode_plugin_protocol::capability::SECRET_WRITE,
    gamenode_plugin_protocol::capability::SECRET_DELETE,
    gamenode_plugin_protocol::capability::COUNTER,
    gamenode_plugin_protocol::capability::ACTIONS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_grants_a_cookie_free_login_without_achievements() {
        assert_eq!(PLUGIN_ID, "org.gamenode.plugin.epic");
        assert_eq!(PROVIDER_ID, "epic");
        assert!(CAPABILITIES.contains(&gamenode_plugin_protocol::capability::PAGE));
        assert!(!CAPABILITIES.contains(&gamenode_plugin_protocol::capability::COOKIES));
        assert!(!CAPABILITIES.contains(&gamenode_plugin_protocol::capability::ACHIEVEMENTS));
    }
}
