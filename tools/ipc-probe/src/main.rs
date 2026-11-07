#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), gamenode_ipc_probe::Error> {
    use gamenode_ipc_probe::{RunningProbe, connect, ping, read_token};
    use std::path::PathBuf;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: gamenode-ipc-probe <serve|ping|ping-without-token|ping-wrong-token> <absolute-directory>".into());
    }
    let dir = PathBuf::from(&args[1]);
    match args[0].to_str() {
        Some("serve") => {
            let setup_dir = dir.clone();
            let server =
                tokio::task::spawn_blocking(move || RunningProbe::bind(&setup_dir)).await??;
            println!(
                "Probe ready at {}; Ctrl+C stops it. No application data is opened.",
                dir.display()
            );
            server
                .serve(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Some(mode @ ("ping" | "ping-without-token" | "ping-wrong-token")) => {
            let token = match mode {
                "ping" => Some(tokio::task::spawn_blocking(move || read_token(&dir)).await??),
                "ping-wrong-token" => Some("0".repeat(64)),
                _ => None,
            };
            // args retain the endpoint even when dir was moved into the blocking task.
            let mut client = connect(&PathBuf::from(&args[1])).await?;
            let reply = ping(&mut client, token.as_deref()).await?;
            println!("OK transport={} uid={}", reply.transport, reply.uid);
        }
        _ => return Err("unknown probe command".into()),
    }
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    eprintln!("This probe validates Unix sockets only; Windows transport is not validated.");
    std::process::exit(1);
}
