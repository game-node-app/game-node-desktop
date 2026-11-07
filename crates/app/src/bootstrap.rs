//! Shared application bootstrap for the Tauri desktop host.
//!
//! This module owns no UI types: callers provide a snapshot publisher and retain
//! the paths/settings they need for their own host lifecycle.

use std::sync::Arc;

use gamenode_core::library::LibraryStore;
use gamenode_core::ports::SettingsStore;
use gamenode_platform::{
    browser::SystemBrowser, credentials, db::SqliteSettingsStore, paths::DataPaths, random,
};

use crate::engine::{Engine, Ports};

pub struct Application {
    pub engine: Engine,
    pub paths: DataPaths,
    pub initial_settings: gamenode_core::Settings,
}

pub fn build(
    publish: impl Fn(gamenode_core::Snapshot) + Send + Sync + 'static,
    runtime: tokio::runtime::Handle,
) -> Result<Application, Box<dyn std::error::Error>> {
    let paths = match std::env::var_os("GAMENODE_DATA_DIR") {
        Some(path) if !path.is_empty() => DataPaths::at(path),
        _ => DataPaths::resolve().ok_or("Could not locate your application data directory")?,
    };
    paths.ensure_data_dir()?;
    let store = Arc::new(SqliteSettingsStore::open(&paths.database_file())?);
    let initial_settings = store.load()?;

    let platform = gamenode_core::provider::Platform::current()
        .ok_or("GameNode Desktop supports Windows and Linux only")?;
    let providers = crate::providers::load_bundled(platform).providers;
    let library_store = Arc::new(store.library_store());
    let layout = library_store.load_layout().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not read the saved library layout; using the default");
        Default::default()
    });
    let library = gamenode_core::library::LibraryMachine::new(providers, layout);

    let credentials = {
        let paths = paths.clone();
        std::thread::spawn(move || credentials::open_store(&paths))
            .join()
            .map_err(|_| "Credential storage initialization failed")?
    };

    let client = Arc::new(gamenode_api::GameNodeClient::new(
        gamenode_api::server_url_from_env()?,
    )?);
    let processes: Arc<dyn gamenode_core::ports::Processes> =
        Arc::new(gamenode_platform::processes::HostProcesses::new());

    let engine = Engine::new(
        Ports {
            backups: Some(Arc::new(gamenode_platform::backups::Ludusavi::bundled(
                paths.data_dir(),
            )?)),
            local_games: Some(Arc::new(store.local_game_store())),
            installations: Some(Arc::new(
                gamenode_platform::installations::WindowsInstallations,
            )),
            tools: Some(Arc::new(store.tool_store())),
            tool_catalog: Some(Arc::new(
                gamenode_platform::tools::catalog::GitHubToolCatalog::new(paths.tools_cache_dir())?,
            )),
            tool_installer: Some(Arc::new(
                gamenode_platform::tools::PlatformToolInstaller::new(
                    paths.tool_backups_dir(),
                    processes.clone(),
                ),
            )),
            processes,
            playtime: Arc::new(store.playtime_store()),
            playtime_api: client.clone(),
            settings: store,
            credentials,
            api: client.clone(),
            browser: Arc::new(SystemBrowser),
            library: library_store,
            games: client,
            files: Arc::new(gamenode_platform::host::StdHostFiles),
            roots: Arc::new(gamenode_platform::host::SystemRoots::new()),
            uri: Arc::new(gamenode_platform::uri::SystemUriOpener),
            covers_dir: paths.covers_dir(),
            platform,
        },
        runtime,
        initial_settings,
        library,
        Box::new(random::nonce),
        publish,
    );

    Ok(Application {
        engine,
        paths,
        initial_settings,
    })
}
