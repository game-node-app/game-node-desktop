#![cfg(unix)]

use gamenode_ipc_probe::{RunningProbe, connect, ping, read_token};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::time::Duration;
use tokio::sync::oneshot;
use tonic::Code;

fn start(dir: &Path) -> (oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let server = RunningProbe::bind(dir).unwrap();
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        server
            .serve(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (stop, task)
}

async fn stop(stop: oneshot::Sender<()>, task: tokio::task::JoinHandle<()>) {
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn grpc_requires_the_instance_token_and_cleans_up_on_shutdown() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("endpoint");
    let (shutdown, task) = start(&dir);
    for name in ["probe.sock", "instance.token", "probe.lock"] {
        assert_eq!(fs::metadata(dir.join(name)).unwrap().mode() & 0o777, 0o600);
    }
    assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    let mut client = connect(&dir).await.unwrap();
    assert_eq!(
        ping(&mut client, None).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        ping(&mut client, Some(&"0".repeat(64)))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    let reply = ping(&mut client, Some(&read_token(&dir).unwrap()))
        .await
        .unwrap();
    assert_eq!(reply.transport, "grpc-over-uds");
    assert_eq!(reply.uid, rustix::process::getuid().as_raw());
    drop(client);
    stop(shutdown, task).await;
    assert!(!dir.join("instance.token").exists());
    assert!(!dir.join("probe.sock").exists());
}

#[tokio::test]
async fn second_owner_is_rejected_and_old_credentials_expire_on_restart() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("endpoint");
    let (shutdown, task) = start(&dir);
    let old_token = read_token(&dir).unwrap();
    assert!(RunningProbe::bind(&dir).is_err());
    assert_eq!(read_token(&dir).unwrap(), old_token);
    let mut first_client = connect(&dir).await.unwrap();
    assert!(ping(&mut first_client, Some(&old_token)).await.is_ok());
    drop(first_client);
    stop(shutdown, task).await;
    let (shutdown, task) = start(&dir);
    let new_token = read_token(&dir).unwrap();
    assert_ne!(new_token, old_token);
    let mut client = connect(&dir).await.unwrap();
    assert_eq!(
        ping(&mut client, Some(&old_token))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert!(ping(&mut client, Some(&new_token)).await.is_ok());
    drop(client);
    stop(shutdown, task).await;
}

#[tokio::test]
async fn unsafe_endpoint_paths_are_refused_without_overwriting_files() {
    let scratch = tempfile::tempdir().unwrap();
    let public = scratch.path().join("public");
    fs::create_dir(&public).unwrap();
    fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(RunningProbe::bind(&public).is_err());
    let linked = scratch.path().join("linked");
    symlink(&public, &linked).unwrap();
    assert!(RunningProbe::bind(&linked).is_err());
    let dir = scratch.path().join("endpoint");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(dir.join("probe.sock"), b"keep this file").unwrap();
    assert!(RunningProbe::bind(&dir).is_err());
    assert_eq!(fs::read(dir.join("probe.sock")).unwrap(), b"keep this file");
    assert!(RunningProbe::bind(Path::new("relative")).is_err());
}

#[tokio::test]
async fn startup_recovers_a_socket_left_by_a_crashed_owner() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("endpoint");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let stale = std::os::unix::net::UnixListener::bind(dir.join("probe.sock")).unwrap();
    drop(stale);
    fs::write(dir.join("instance.token"), "a".repeat(64)).unwrap();
    fs::set_permissions(
        dir.join("instance.token"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let (shutdown, task) = start(&dir);
    let mut client = connect(&dir).await.unwrap();
    assert!(
        ping(&mut client, Some(&read_token(&dir).unwrap()))
            .await
            .is_ok()
    );
    drop(client);
    stop(shutdown, task).await;
}
