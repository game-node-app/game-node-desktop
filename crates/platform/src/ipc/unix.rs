//! Linux single-instance IPC: `flock` election plus a private Unix socket.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream as StdStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustix::fs::OFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc};

use super::protocol::{self, Message};
use super::{Instance, IpcError, MAX_CONNECTIONS, READ_TIMEOUT};

const LOCK_FILE: &str = "instance.lock";
const SOCKET_FILE: &str = "instance.sock";
/// `sockaddr_un.sun_path` holds 108 bytes including the terminator.
const MAX_SOCKET_PATH: usize = 100;

/// Where the lock and socket live.
#[derive(Clone, Debug)]
pub struct Endpoint {
    dir: PathBuf,
}

impl Endpoint {
    /// `$XDG_RUNTIME_DIR/gamenode-desktop`, or a private per-user directory in the temp dir.
    pub fn for_current_user() -> Result<Self, IpcError> {
        let dir = match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
            Some(runtime) if runtime.is_absolute() => runtime.join("gamenode-desktop"),
            _ => std::env::temp_dir().join(format!(
                "gamenode-desktop-{}",
                rustix::process::getuid().as_raw()
            )),
        };
        Ok(Self { dir })
    }

    /// An explicit directory (tests).
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn socket_path(&self) -> PathBuf {
        self.dir.join(SOCKET_FILE)
    }

    pub fn lock_path(&self) -> PathBuf {
        self.dir.join(LOCK_FILE)
    }

    fn ensure_dir(&self) -> Result<(), IpcError> {
        let unsafe_dir = |why: &str| IpcError::Location(why.to_owned());
        if self.socket_path().as_os_str().len() > MAX_SOCKET_PATH {
            return Err(unsafe_dir("socket path too long"));
        }
        match fs::symlink_metadata(&self.dir) {
            Ok(meta) => {
                if !meta.is_dir() || meta.uid() != rustix::process::getuid().as_raw() {
                    return Err(unsafe_dir("not a directory owned by the current user"));
                }
                if meta.mode() & 0o077 != 0 {
                    fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
                        .map_err(|e| unsafe_dir(&e.to_string()))?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = self.dir.parent() {
                    fs::create_dir_all(parent).map_err(|e| unsafe_dir(&e.to_string()))?;
                }
                match fs::DirBuilder::new().mode(0o700).create(&self.dir) {
                    Ok(()) => {}
                    // Another launch may have created it after our metadata check.
                    // Revalidate ownership and permissions before using it.
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        return self.ensure_dir();
                    }
                    Err(e) => return Err(unsafe_dir(&e.to_string())),
                }
            }
            Err(e) => return Err(unsafe_dir(&e.to_string())),
        }
        Ok(())
    }

    pub(super) fn acquire(&self, runtime: &tokio::runtime::Handle) -> Result<Instance, IpcError> {
        self.ensure_dir()?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .custom_flags(OFlags::NOFOLLOW.bits() as i32)
            .open(self.lock_path())
            .map_err(IpcError::Bind)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Ok(Instance::Existing(Existing {
                    socket: self.socket_path(),
                }));
            }
            Err(TryLockError::Error(e)) => return Err(IpcError::Bind(e)),
        }
        // We hold the lock, so any socket file belongs to a crashed owner.
        match fs::remove_file(self.socket_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(IpcError::Bind(e)),
        }
        let listener =
            std::os::unix::net::UnixListener::bind(self.socket_path()).map_err(IpcError::Bind)?;
        fs::set_permissions(self.socket_path(), fs::Permissions::from_mode(0o600))
            .map_err(IpcError::Bind)?;
        listener.set_nonblocking(true).map_err(IpcError::Bind)?;
        let _guard = runtime.enter();
        let listener = UnixListener::from_std(listener).map_err(IpcError::Bind)?;
        Ok(Instance::Owner(Owner {
            _lock: lock,
            listener,
            socket: self.socket_path(),
        }))
    }
}

/// The running application's endpoint. Dropping it releases single-instance ownership.
pub struct Owner {
    _lock: File,
    listener: UnixListener,
    socket: PathBuf,
}

impl Owner {
    /// Starts accepting messages on `runtime`; returns the receiving end. Valid messages are
    /// queued (bounded); malformed, oversized, slow or foreign-user connections are dropped.
    pub fn serve(self, runtime: &tokio::runtime::Handle) -> OwnerGuard {
        let (tx, rx) = mpsc::channel(32);
        let socket = self.socket.clone();
        let task = runtime.spawn(accept_loop(self.listener, tx));
        OwnerGuard {
            messages: rx,
            task,
            socket,
            _lock: self._lock,
        }
    }
}

/// A serving owner. Dropping it stops serving and removes the socket.
pub struct OwnerGuard {
    pub messages: mpsc::Receiver<Message>,
    task: tokio::task::JoinHandle<()>,
    socket: PathBuf,
    _lock: File,
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        self.task.abort();
        let _ = fs::remove_file(&self.socket);
    }
}

async fn accept_loop(listener: UnixListener, tx: mpsc::Sender<Message>) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let our_uid = rustix::process::getuid().as_raw();
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        if !stream.peer_cred().is_ok_and(|c| c.uid() == our_uid) {
            continue;
        }
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let _permit = permit;
            handle(stream, tx).await;
        });
    }
}

async fn handle(mut stream: UnixStream, tx: mpsc::Sender<Message>) {
    let received = tokio::time::timeout(READ_TIMEOUT, async {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await.ok()?;
        let len = protocol::body_len(header).ok()?;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        protocol::decode(&body).ok()
    })
    .await
    .ok()
    .flatten();
    let accepted = match received {
        Some(message) => tx.try_send(message).is_ok(),
        None => false,
    };
    let _ = stream
        .write_all(&[if accepted {
            protocol::ACK
        } else {
            protocol::NACK
        }])
        .await;
}

/// Handle to another process that owns the application session.
pub struct Existing {
    socket: PathBuf,
}

impl Existing {
    /// Sends `message` and waits for acceptance, retrying until `wait` while the owner starts.
    pub fn forward(&self, message: &Message, wait: Duration) -> Result<(), IpcError> {
        let deadline = Instant::now() + wait;
        let frame = protocol::encode(message);
        loop {
            match self.try_forward(&frame) {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(IpcError::Rejected),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(_) => return Err(IpcError::Unreachable),
            }
        }
    }

    fn try_forward(&self, frame: &[u8]) -> std::io::Result<bool> {
        let mut stream = StdStream::connect(&self.socket)?;
        stream.set_write_timeout(Some(READ_TIMEOUT))?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.write_all(frame)?;
        let mut ack = [0u8; 1];
        stream.read_exact(&mut ack)?;
        Ok(ack[0] == protocol::ACK)
    }
}
