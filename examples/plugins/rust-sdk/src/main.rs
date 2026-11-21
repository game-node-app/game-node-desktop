use std::sync::Arc;

use gamenode_plugin_mvp_example::{
    CAPABILITIES, MvpProvider, PLUGIN_ID, PLUGIN_VERSION, PROVIDER_ID,
};
use gamenode_plugin_sdk::{Launch, Session, SessionOptions, proto::SessionKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = Session::open_with_options(
        Launch::from_env()?,
        PLUGIN_ID,
        PROVIDER_ID,
        PLUGIN_VERSION,
        CAPABILITIES,
        SessionOptions {
            min_api_major: 1,
            min_api_minor: 6,
            max_api_major: 1,
            max_api_minor: 6,
            features: vec![],
            session_kind: SessionKind::Provider,
            target_game: None,
        },
    )
    .await?;

    gamenode_plugin_sdk::run(session, Arc::new(MvpProvider)).await?;
    Ok(())
}
