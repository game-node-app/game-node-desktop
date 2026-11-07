use gamenode_platform::ipc::{self, Instance, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("GameNode background daemon (normally started by gamenode-cli)");
        return Ok(());
    }
    #[cfg(not(unix))]
    return Err("the first daemon transport currently requires Unix domain sockets".into());
    #[cfg(unix)]
    run().await
}

#[cfg(unix)]
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ipc::Endpoint::for_current_user()?;
    let owner = match ipc::acquire(&endpoint, &tokio::runtime::Handle::current())? {
        Instance::Owner(owner) => owner,
        Instance::Existing(_) => return Err("GameNode daemon is already running".into()),
    };
    let mut owner = owner.serve(&tokio::runtime::Handle::current());
    let socket = endpoint.dir().join("client.sock");
    let runtime = tokio::runtime::Handle::current();
    let app = tokio::task::spawn_blocking(move || {
        gamenode_app::bootstrap::build(|_| {}, runtime).map_err(|error| error.to_string())
    })
    .await??;
    let engine = app.engine;
    engine.start();
    let callback_engine = engine.clone();
    let maintenance_engine = engine.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            maintenance_engine.dispatch(gamenode_core::Input::Tick);
            maintenance_engine.maintenance();
        }
    });
    tokio::spawn(async move {
        while let Some(message) = owner.messages.recv().await {
            if let Message::AuthCallback { url } = message {
                callback_engine.dispatch(gamenode_core::Input::CallbackReceived(url));
            }
        }
    });
    gamenode_app::daemon::serve(engine.clone(), socket).await?;
    engine.shutdown();
    Ok(())
}
