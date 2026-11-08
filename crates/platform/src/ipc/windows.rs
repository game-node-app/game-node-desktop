//! Windows single-instance IPC: a per-user named pipe.
//!
//! The first pipe instance (`FILE_FLAG_FIRST_PIPE_INSTANCE`) elects the owner. The pipe name
//! includes the user's SID and its DACL admits only that user and SYSTEM; remote clients are
//! rejected.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use tokio::sync::{Semaphore, mpsc};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::protocol::{self, Message};
use super::{Instance, IpcError, MAX_CONNECTIONS, READ_TIMEOUT};

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_PIPE_BUSY: i32 = 231;

#[derive(Clone, Debug)]
pub struct Endpoint {
    path: String,
    sddl: String,
}

impl Endpoint {
    pub fn for_current_user() -> Result<Self, IpcError> {
        let sid = current_user_sid().map_err(|e| IpcError::Location(e.to_string()))?;
        Ok(Self::build(format!("gamenode-desktop-{sid}"), &sid))
    }

    /// An explicitly named pipe for the current user (tests).
    pub fn named(name: &str) -> Result<Self, IpcError> {
        let sid = current_user_sid().map_err(|e| IpcError::Location(e.to_string()))?;
        Ok(Self::build(name.to_owned(), &sid))
    }

    /// The full pipe path (tests).
    pub fn pipe_path(&self) -> &str {
        &self.path
    }

    fn build(name: String, sid: &str) -> Self {
        Self {
            path: format!(r"\\.\pipe\{name}"),
            // Protected DACL: full access for the current user and SYSTEM only.
            sddl: format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)"),
        }
    }

    pub(super) fn acquire(&self, runtime: &tokio::runtime::Handle) -> Result<Instance, IpcError> {
        let _guard = runtime.enter();
        match create_instance(&self.path, &self.sddl, true) {
            Ok(server) => Ok(Instance::Owner(Owner {
                server,
                endpoint: self.clone(),
            })),
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
                ) =>
            {
                Ok(Instance::Existing(Existing {
                    path: self.path.clone(),
                }))
            }
            Err(e) => Err(IpcError::Bind(e)),
        }
    }
}

fn create_instance(path: &str, sddl: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `descriptor` receives a
    // LocalAlloc'd buffer that is freed below after the pipe has been created.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` points at a valid descriptor and lives across the call.
    let server = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            )
    };
    // SAFETY: `descriptor` came from the successful conversion above and is not used afterwards.
    unsafe { LocalFree(descriptor) };
    server
}

fn current_user_sid() -> std::io::Result<String> {
    // SAFETY: standard two-call TOKEN_USER query; every pointer is valid for its call, the
    // buffer is 8-byte aligned for TOKEN_USER, and each OS allocation is released.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
            let mut text: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            Ok(sid)
        })();
        CloseHandle(token);
        result
    }
}

/// A separate authenticated-service transport using the same user/SYSTEM DACL
/// as single-instance IPC. Each accepted connection receives a fresh instance.
pub struct PrivatePipeListener {
    server: NamedPipeServer,
    path: String,
    sddl: String,
}

impl PrivatePipeListener {
    /// Call with the Tokio reactor entered. The supplied instance suffix is
    /// host-generated, never an arbitrary path from a plugin.
    pub fn bind(instance: &str) -> std::io::Result<Self> {
        if instance.is_empty()
            || instance.len() > 96
            || !instance
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid pipe instance",
            ));
        }
        let sid = current_user_sid()?;
        let endpoint = Endpoint::build(format!("gamenode-plugins-{sid}-{instance}"), &sid);
        let server = create_instance(&endpoint.path, &endpoint.sddl, true)?;
        Ok(Self {
            server,
            path: endpoint.path,
            sddl: endpoint.sddl,
        })
    }

    pub fn pipe_path(&self) -> &str {
        &self.path
    }

    pub async fn accept(&mut self) -> std::io::Result<NamedPipeServer> {
        self.server.connect().await?;
        // Keep ownership continuously: create the next listening instance
        // before the accepted instance can be dropped by its connection owner.
        let next = create_instance(&self.path, &self.sddl, false)?;
        Ok(std::mem::replace(&mut self.server, next))
    }
}

pub struct Owner {
    server: NamedPipeServer,
    endpoint: Endpoint,
}

impl Owner {
    pub fn serve(self, runtime: &tokio::runtime::Handle) -> OwnerGuard {
        let (tx, rx) = mpsc::channel(32);
        let task = runtime.spawn(accept_loop(self.server, self.endpoint, tx));
        OwnerGuard { messages: rx, task }
    }
}

/// A serving owner. Dropping it stops serving; the pipe vanishes with the last handle.
pub struct OwnerGuard {
    pub messages: mpsc::Receiver<Message>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept_loop(first: NamedPipeServer, endpoint: Endpoint, tx: mpsc::Sender<Message>) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut server = first;
    loop {
        if server.connect().await.is_err() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        } else {
            let connected = server;
            server = loop {
                match create_instance(&endpoint.path, &endpoint.sddl, false) {
                    Ok(next) => break next,
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            };
            if let Ok(permit) = permits.clone().try_acquire_owned() {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    handle(connected, tx).await;
                });
            }
            continue;
        }
        // The failed instance may be unusable: replace it.
        if let Ok(next) = create_instance(&endpoint.path, &endpoint.sddl, false) {
            server = next;
        }
    }
}

async fn handle(mut stream: NamedPipeServer, tx: mpsc::Sender<Message>) {
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
    let accepted = received.is_some_and(|message| tx.try_send(message).is_ok());
    let _ = stream
        .write_all(&[if accepted {
            protocol::ACK
        } else {
            protocol::NACK
        }])
        .await;
    let _ = stream.flush().await;
    // Let the client read the reply before the handle closes (closing discards unread data).
    let _ = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut [0u8; 1])).await;
}

pub struct Existing {
    path: String,
}

impl Existing {
    /// Runs on the launch thread or a blocking worker. An overlapped pipe allows
    /// the deadline to cancel a read even when the owner has stopped responding.
    pub fn forward(&self, message: &Message, wait: Duration) -> Result<(), IpcError> {
        let frame = protocol::encode(message);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(IpcError::Bind)?;
        runtime.block_on(async {
            tokio::time::timeout(wait, async {
                let mut pipe = loop {
                    match ClientOptions::new().open(&self.path) {
                        Ok(pipe) => break pipe,
                        Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                    }
                };
                pipe.write_all(&frame)
                    .await
                    .map_err(|_| IpcError::Unreachable)?;
                let mut ack = [0u8; 1];
                pipe.read_exact(&mut ack)
                    .await
                    .map_err(|_| IpcError::Unreachable)?;
                if ack[0] == protocol::ACK {
                    Ok(())
                } else {
                    Err(IpcError::Rejected)
                }
            })
            .await
            .map_err(|_| IpcError::Unreachable)?
        })
    }
}

#[cfg(test)]
mod plugin_pipe_tests {
    use super::*;

    #[tokio::test]
    async fn private_listener_owns_the_name_and_accepts_repeated_connections() {
        let name = format!("test-{}", std::process::id());
        let mut listener = PrivatePipeListener::bind(&name).unwrap();
        assert!(listener.path.contains(&current_user_sid().unwrap()));
        assert!(PrivatePipeListener::bind(&name).is_err());
        for _ in 0..2 {
            let mut client = ClientOptions::new().open(listener.pipe_path()).unwrap();
            let mut accepted = listener.accept().await.unwrap();
            client.write_all(b"test").await.unwrap();
            let mut data = [0; 4];
            accepted.read_exact(&mut data).await.unwrap();
            assert_eq!(&data, b"test");
        }
        assert!(PrivatePipeListener::bind(&name).is_err());
        drop(listener);
        assert!(PrivatePipeListener::bind(&name).is_ok());
    }
}
