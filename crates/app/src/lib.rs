pub mod commands;
pub mod engine;
pub mod logging;
pub mod plugin_host;
pub mod protocol;
pub mod providers;

/// The opt-in allocator enabled by the `mimalloc` feature. Default builds are unaffected.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
