//! Test helper for the single-instance process tests. Not part of the shipped application.
//!
//! `instance_probe --endpoint <dir-or-pipe-name> [--serve-ms N] [--delay-serve-ms N] [--crash] [--callback URL]`
//!
//! Prints `OWNER` if it became the owner (then serves for `--serve-ms`, printing one
//! `MSG <json>` line per received message; `--crash` aborts instead, leaving its endpoint stale)
//! or `FORWARDED` after handing a message to the existing owner.

use std::io::Write;
use std::time::{Duration, Instant};

use gamenode_platform::ipc::{self, Endpoint, FORWARD_WAIT, Instance, Message};

fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut endpoint, mut serve_ms, mut crash, mut callback) = (None, 1500u64, false, None);
    let mut delay_ms = 0u64;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--endpoint" => endpoint = args.next(),
            "--serve-ms" => serve_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(serve_ms),
            "--delay-serve-ms" => delay_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--crash" => crash = true,
            "--callback" => callback = args.next(),
            other => panic!("unknown argument {other}"),
        }
    }
    let endpoint = endpoint.expect("--endpoint is required");
    #[cfg(unix)]
    let endpoint = Endpoint::in_dir(endpoint);
    #[cfg(windows)]
    let endpoint = Endpoint::named(&endpoint).expect("pipe endpoint");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    match ipc::acquire(&endpoint, runtime.handle()).expect("acquire") {
        Instance::Owner(owner) => {
            say("OWNER");
            if crash {
                // No destructors run: the socket file (Unix) stays behind.
                std::process::abort();
            }
            // Simulates an owner that has claimed the endpoint but is still starting up.
            std::thread::sleep(Duration::from_millis(delay_ms));
            let mut guard = owner.serve(runtime.handle());
            let deadline = Instant::now() + Duration::from_millis(serve_ms);
            runtime.block_on(async {
                while let Ok(Some(message)) =
                    tokio::time::timeout_at(deadline.into(), guard.messages.recv()).await
                {
                    say(&format!("MSG {}", serde_json::to_string(&message).unwrap()));
                }
            });
        }
        Instance::Existing(existing) => {
            let message = match callback {
                Some(url) => Message::AuthCallback { url },
                None => Message::Activate,
            };
            match existing.forward(&message, FORWARD_WAIT) {
                Ok(()) => say("FORWARDED"),
                Err(_) => {
                    say("FAILED");
                    std::process::exit(1);
                }
            }
        }
    }
}
