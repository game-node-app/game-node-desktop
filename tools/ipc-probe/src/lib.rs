//! Isolated Unix transport/authentication experiment; no access to application data.
#![cfg(unix)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hyper_util::rt::TokioIo;
use subtle::ConstantTimeEq;
use tokio::net::{UnixListener, UnixStream};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Request, Response, Status};
use tower::service_fn;

pub mod wire {
    tonic::include_proto!("gamenode.probe.v1");
}
use wire::probe_client::ProbeClient;
use wire::probe_server::{Probe, ProbeServer};
use wire::{PingReply, PingRequest};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
const TIMEOUT: Duration = Duration::from_secs(3);

fn refuse(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn verify_private(path: &Path, directory: bool) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let valid_type = if directory {
        meta.is_dir()
    } else {
        meta.is_file()
    };
    if !valid_type || meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(refuse(
            "endpoint must be owned by this user and inaccessible to others",
        ));
    }
    Ok(())
}

fn socket_path(dir: &Path) -> io::Result<PathBuf> {
    if !dir.is_absolute() {
        return Err(refuse("use an absolute directory"));
    }
    let path = dir.join("probe.sock");
    if path.as_os_str().len() > 100 {
        return Err(refuse(
            "Unix socket path exceeds the probe's 100-byte limit",
        ));
    }
    Ok(path)
}

fn verify_peer(stream: &UnixStream) -> io::Result<()> {
    if stream.peer_cred()?.uid() != rustix::process::getuid().as_raw() {
        return Err(refuse("peer UID differs from the current user"));
    }
    Ok(())
}

pub struct RunningProbe {
    listener: Option<UnixListener>,
    socket: PathBuf,
    token_path: PathBuf,
    token: String,
    // Held until after socket/token cleanup, including on cancellation or shutdown.
    _lock: File,
}

impl RunningProbe {
    /// Blocking filesystem setup: call outside UI callbacks or on a blocking worker.
    pub fn bind(dir: &Path) -> Result<Self, Error> {
        let socket = socket_path(dir)?;
        match fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        verify_private(dir, true)?;
        let lock_path = dir.join("probe.lock");
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&lock_path)?;
        verify_private(&lock_path, false)?;
        lock.try_lock()
            .map_err(|_| refuse("probe already active; lock acquisition refused"))?;
        // Only the lock owner can replace files left by a terminated instance.
        match fs::symlink_metadata(&socket) {
            Ok(meta)
                if meta.file_type().is_socket()
                    && meta.uid() == rustix::process::getuid().as_raw() =>
            {
                fs::remove_file(&socket)?
            }
            Ok(_) => return Err(refuse("refusing to replace a non-socket endpoint").into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let token_path = dir.join("instance.token");
        match fs::symlink_metadata(&token_path) {
            Ok(_) => {
                verify_private(&token_path, false)?;
                fs::remove_file(&token_path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let listener = UnixListener::bind(&socket)?;
        let probe = Self {
            listener: Some(listener),
            socket,
            token_path,
            token,
            _lock: lock,
        };
        fs::set_permissions(&probe.socket, fs::Permissions::from_mode(0o600))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe.token_path)?;
        file.write_all(probe.token.as_bytes())?;
        Ok(probe)
    }

    pub async fn serve(
        mut self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), Error> {
        let expected = self.token.clone();
        let service = ProbeServer::with_interceptor(ProbeService, move |request: Request<()>| {
            let presented = request
                .metadata()
                .get("authorization")
                .map(|v| v.as_encoded_bytes())
                .unwrap_or_default();
            let expected = format!("Bearer {expected}");
            if !bool::from(presented.ct_eq(expected.as_bytes())) {
                return Err(Status::unauthenticated(
                    "missing or invalid instance credential",
                ));
            }
            Ok(request)
        });
        let incoming = UnixListenerStream::new(self.listener.take().expect("listener present"))
            .filter_map(|stream| match stream {
                Ok(stream) if verify_peer(&stream).is_ok() => Some(Ok::<_, io::Error>(stream)),
                _ => None,
            });
        Server::builder()
            .timeout(TIMEOUT)
            .add_service(service)
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await?;
        Ok(())
    }
}

impl Drop for RunningProbe {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_file(&self.token_path);
    }
}

struct ProbeService;
#[tonic::async_trait]
impl Probe for ProbeService {
    async fn ping(&self, _: Request<PingRequest>) -> Result<Response<PingReply>, Status> {
        Ok(Response::new(PingReply {
            transport: "grpc-over-uds".into(),
            uid: rustix::process::getuid().as_raw(),
        }))
    }
}

pub fn read_token(dir: &Path) -> Result<String, Error> {
    verify_private(dir, true)?;
    let path = dir.join("instance.token");
    verify_private(&path, false)?;
    let token = fs::read_to_string(path)?;
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(refuse("invalid instance credential file").into());
    }
    Ok(token)
}

pub async fn connect(dir: &Path) -> Result<ProbeClient<Channel>, Error> {
    verify_private(dir, true)?;
    let path = socket_path(dir)?;
    let channel = Endpoint::from_static("http://[::]:50051")
        .connect_timeout(TIMEOUT)
        .timeout(TIMEOUT)
        .connect_with_connector(service_fn(move |_| {
            let path = path.clone();
            async move {
                let stream = UnixStream::connect(path).await?;
                verify_peer(&stream)?;
                Ok::<_, io::Error>(TokioIo::new(stream))
            }
        }))
        .await?;
    Ok(ProbeClient::new(channel))
}

pub async fn ping(
    client: &mut ProbeClient<Channel>,
    token: Option<&str>,
) -> Result<PingReply, Status> {
    let mut request = Request::new(PingRequest {});
    request.set_timeout(TIMEOUT);
    if let Some(token) = token {
        let value = format!("Bearer {token}")
            .parse()
            .map_err(|_| Status::invalid_argument("invalid credential format"))?;
        request.metadata_mut().insert("authorization", value);
    }
    Ok(client.ping(request).await?.into_inner())
}
