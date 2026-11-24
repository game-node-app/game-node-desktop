#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    if let Err(code) = linux::run().await {
        tracing::warn!(failure = code, "Linux package manager plugin failed");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
