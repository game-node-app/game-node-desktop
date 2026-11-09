#![cfg_attr(windows, windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .try_init();
    let result = provider().await;
    if let Err(code) = result {
        tracing::warn!(failure = code, "Steam provider plugin failed");
        std::process::exit(1);
    }
}

async fn provider() -> Result<(), &'static str> {
    use gamenode_plugin_sdk::{Launch, Session};
    let launch = Launch::from_env().map_err(|_| "invalid_launch_configuration")?;
    let session = Session::open(
        launch,
        "org.gamenode.plugin.steam-discovery",
        "steam",
        env!("CARGO_PKG_VERSION"),
        gamenode_plugin_protocol::capability::ALL,
    )
    .await
    .map_err(|_| "provider_session_unavailable")?;
    let plugin = gamenode_plugin_steam::runtime::SteamPlugin::new()
        .map_err(|_| "provider_initialization_failed")?;
    gamenode_plugin_sdk::run(session, std::sync::Arc::new(plugin))
        .await
        .map_err(|_| "provider_session_failed")
}
