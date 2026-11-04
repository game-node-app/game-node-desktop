#[cfg(windows)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use super::*;

/// A fresh endpoint per test. The guard keeps its directory alive on Unix.
struct Fixture {
    endpoint: Endpoint,
    _dir: Option<tempfile::TempDir>,
}

fn fixture() -> Fixture {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        // A subdirectory: the endpoint creates (and secures) it itself.
        Fixture {
            endpoint: Endpoint::in_dir(dir.path().join("gn")),
            _dir: Some(dir),
        }
    }
    #[cfg(windows)]
    {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "gamenode-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        );
        Fixture {
            endpoint: Endpoint::named(&name).unwrap(),
            _dir: None,
        }
    }
}

const WAIT: Duration = Duration::from_secs(3);

fn owner(f: &Fixture) -> Owner {
    match acquire(&f.endpoint, &tokio::runtime::Handle::current()).unwrap() {
        Instance::Owner(owner) => owner,
        Instance::Existing(_) => panic!("expected to become the owner"),
    }
}

async fn forward(f: &Fixture, message: Message) -> Result<(), IpcError> {
    let Instance::Existing(existing) =
        acquire(&f.endpoint, &tokio::runtime::Handle::current()).unwrap()
    else {
        panic!("expected an existing owner");
    };
    tokio::task::spawn_blocking(move || existing.forward(&message, WAIT))
        .await
        .unwrap()
}

/// Writes raw bytes to the endpoint like a hostile local process would, returning the reply byte.
fn raw_exchange(f: &Fixture, bytes: &[u8], hold: bool) -> Option<u8> {
    let _ = hold;
    #[cfg(unix)]
    let mut stream = std::os::unix::net::UnixStream::connect(f.endpoint.socket_path()).ok()?;
    #[cfg(windows)]
    let mut stream = {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(f.endpoint.pipe_path())
            .ok()?
    };
    use std::io::{Read, Write};
    #[cfg(unix)]
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream.write_all(bytes).ok()?;
    let mut reply = [0u8; 1];
    stream.read_exact(&mut reply).ok()?;
    Some(reply[0])
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_receives_activation_and_an_intact_callback() {
    let f = fixture();
    let mut guard = owner(&f).serve(&tokio::runtime::Handle::current());

    forward(&f, Message::Activate).await.unwrap();
    assert_eq!(guard.messages.recv().await, Some(Message::Activate));

    // Spaces, encoded data and shell syntax travel as data, unchanged.
    let url = r#"app.gamenode://home?desktop_attempt=a b&cookie=%E2%9C%93%3B&x=$(rm -rf ~)"`;|&"#;
    forward(&f, Message::AuthCallback { url: url.into() })
        .await
        .unwrap();
    assert_eq!(
        guard.messages.recv().await,
        Some(Message::AuthCallback { url: url.into() })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_launch_finds_the_owner_and_ownership_is_released_on_exit() {
    let f = fixture();
    let guard = owner(&f).serve(&tokio::runtime::Handle::current());
    assert!(matches!(
        acquire(&f.endpoint, &tokio::runtime::Handle::current()).unwrap(),
        Instance::Existing(_)
    ));
    drop(guard);
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The closed application no longer owns anything: a new launch becomes the owner.
    let _again = owner(&f);
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_acquisition_elects_exactly_one_owner() {
    let f = fixture();
    let handle = tokio::runtime::Handle::current();
    let results: Vec<_> = (0..8)
        .map(|_| {
            let endpoint = f.endpoint.clone();
            let handle = handle.clone();
            std::thread::spawn(move || acquire(&endpoint, &handle).unwrap())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();
    let owners = results
        .iter()
        .filter(|r| matches!(r, Instance::Owner(_)))
        .count();
    assert_eq!(owners, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn hostile_or_malformed_payloads_are_rejected_and_the_owner_keeps_working() {
    let f = fixture();
    let mut guard = owner(&f).serve(&tokio::runtime::Handle::current());
    let frame = |body: &[u8]| {
        let mut v = (body.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(body);
        v
    };
    let attacks: Vec<Vec<u8>> = vec![
        frame(b"not json"),
        frame(br#"{"type":"auth-callback","url":"app.gamenode://home","extra":true}"#),
        frame(br#"{"type":"run","command":"calc.exe"}"#),
        u32::MAX.to_be_bytes().to_vec(), // absurd length
        (MAX_FRAME as u32 + 1).to_be_bytes().to_vec(), // just over the bound
        0u32.to_be_bytes().to_vec(),     // empty
        vec![0, 0],                      // truncated header, then EOF
    ];
    for attack in attacks {
        let reply = tokio::task::spawn_blocking({
            let endpoint = f.endpoint.clone();
            move || {
                let f = Fixture {
                    endpoint,
                    _dir: None,
                };
                // Closing early (truncated input) yields no reply at all.
                raw_exchange(&f, &attack, false)
            }
        })
        .await
        .unwrap();
        assert_ne!(reply, Some(ACK), "hostile input was acknowledged");
    }
    assert!(
        guard.messages.try_recv().is_err(),
        "no hostile message reached the application"
    );

    forward(&f, Message::Activate).await.unwrap();
    assert_eq!(guard.messages.recv().await, Some(Message::Activate));
}

#[tokio::test(flavor = "multi_thread")]
async fn forwarding_without_a_responsive_owner_fails_after_the_wait() {
    let f = fixture();
    // Hold ownership without serving: connections are never answered.
    let _held = owner(&f);
    let Instance::Existing(existing) =
        acquire(&f.endpoint, &tokio::runtime::Handle::current()).unwrap()
    else {
        panic!()
    };
    let started = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        existing.forward(&Message::Activate, Duration::from_millis(300))
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(IpcError::Unreachable)));
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[cfg(unix)]
mod unix_only {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test(flavor = "multi_thread")]
    async fn directory_and_socket_are_private() {
        let f = fixture();
        let _guard = owner(&f).serve(&tokio::runtime::Handle::current());
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(f.endpoint.dir()), 0o700);
        assert_eq!(mode(&f.endpoint.socket_path()), 0o600);
        assert_eq!(mode(&f.endpoint.lock_path()), 0o600);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stale_socket_from_a_crashed_owner_is_replaced() {
        let f = fixture();
        // Simulate a crash: a socket file with no lock holder.
        std::fs::create_dir_all(f.endpoint.dir()).unwrap();
        drop(std::os::unix::net::UnixListener::bind(f.endpoint.socket_path()).unwrap());
        assert!(f.endpoint.socket_path().exists());
        let mut guard = owner(&f).serve(&tokio::runtime::Handle::current());
        forward(&f, Message::Activate).await.unwrap();
        assert_eq!(guard.messages.recv().await, Some(Message::Activate));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unsafe_runtime_locations_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("gn");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let endpoint = Endpoint::in_dir(&link);
        let result = acquire(&endpoint, &tokio::runtime::Handle::current());
        assert!(matches!(result, Err(IpcError::Location(_))));

        let long = Endpoint::in_dir(dir.path().join("x".repeat(120)));
        assert!(matches!(
            acquire(&long, &tokio::runtime::Handle::current()),
            Err(IpcError::Location(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stalled_client_is_dropped_after_the_read_timeout() {
        let f = fixture();
        let mut guard = owner(&f).serve(&tokio::runtime::Handle::current());
        let socket = f.endpoint.socket_path();
        let reply = tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};
            let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream.write_all(&[0, 0]).unwrap(); // half a header, then silence
            let mut reply = [0u8; 1];
            stream.read_exact(&mut reply).map(|_| reply[0])
        })
        .await
        .unwrap();
        assert_eq!(reply.unwrap(), NACK);
        assert!(guard.messages.try_recv().is_err());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn second_background_launch_sends_nothing() {
    let f = fixture();
    let mut guard = owner(&f).serve(&tokio::runtime::Handle::current());
    let Instance::Existing(existing) =
        acquire(&f.endpoint, &tokio::runtime::Handle::current()).unwrap()
    else {
        panic!("expected existing")
    };
    forward_launch(&existing, &Message::Activate, true, WAIT).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), guard.messages.recv())
            .await
            .is_err()
    );
    // Prove the owner still responds to a subsequent ordinary launch.
    forward(&f, Message::Activate).await.unwrap();
    assert_eq!(guard.messages.recv().await, Some(Message::Activate));
}
