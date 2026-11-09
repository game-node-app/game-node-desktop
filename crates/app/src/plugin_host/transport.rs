//! Connection permits follow the actual transport lifetime, rather than merely
//! limiting how many accept futures are polled at once.

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(super) struct Connection<T> {
    pub inner: T,
    pub permit: tokio::sync::OwnedSemaphorePermit,
}

impl<T: Send + Sync + 'static> tonic::transport::server::Connected for Connection<T> {
    type ConnectInfo = ();
    fn connect_info(&self) {
        let _ = &self.permit;
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Connection<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Connection<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn permit_is_held_for_the_connection_lifetime() {
        let capacity = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let (io, mut peer) = tokio::io::duplex(32);
        let mut connection = Connection {
            inner: io,
            permit: capacity.clone().try_acquire_owned().unwrap(),
        };
        assert!(capacity.clone().try_acquire_owned().is_err());
        peer.write_all(b"hello").await.unwrap();
        let mut bytes = [0; 5];
        connection.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"hello");
        drop(connection);
        assert!(capacity.try_acquire_owned().is_ok());
    }
}
