use async_dup::Arc;
use async_dup::Mutex;
use std::fmt::Debug;
use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::io::AsyncWrite;
use tokio::io::DuplexStream;
use tokio::io::Sink;
use tokio::io::Stderr;
use tokio::io::Stdout;
use tokio_util::compat::Compat;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

#[derive(Debug)]
pub enum GhciWriter {
    Stdout(Stdout),
    Stderr(Stderr),
    DuplexStream(Compat<Arc<Mutex<Compat<DuplexStream>>>>),
    Sink(Sink),
}

impl GhciWriter {
    pub fn stdout() -> Self {
        Self::Stdout(tokio::io::stdout())
    }

    pub fn stderr() -> Self {
        Self::Stderr(tokio::io::stderr())
    }

    pub fn duplex_stream(duplex_stream: DuplexStream) -> Self {
        Self::DuplexStream(Arc::new(Mutex::new(duplex_stream.compat_write())).compat_write())
    }

    pub fn sink() -> Self {
        Self::Sink(tokio::io::sink())
    }
}

impl AsyncWrite for GhciWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match Pin::into_inner(self) {
            Self::Stdout(ref mut x) => Pin::new(x).poll_write(cx, buf),
            Self::Stderr(ref mut x) => Pin::new(x).poll_write(cx, buf),
            Self::DuplexStream(ref mut x) => Pin::new(x).poll_write(cx, buf),
            Self::Sink(ref mut x) => Pin::new(x).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match Pin::into_inner(self) {
            Self::Stdout(ref mut x) => Pin::new(x).poll_flush(cx),
            Self::Stderr(ref mut x) => Pin::new(x).poll_flush(cx),
            Self::DuplexStream(ref mut x) => Pin::new(x).poll_flush(cx),
            Self::Sink(ref mut x) => Pin::new(x).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match Pin::into_inner(self) {
            Self::Stdout(ref mut x) => Pin::new(x).poll_shutdown(cx),
            Self::Stderr(ref mut x) => Pin::new(x).poll_shutdown(cx),
            Self::DuplexStream(ref mut x) => Pin::new(x).poll_shutdown(cx),
            Self::Sink(ref mut x) => Pin::new(x).poll_shutdown(cx),
        }
    }
}

impl Clone for GhciWriter {
    fn clone(&self) -> Self {
        match self {
            Self::Stdout(_) => Self::stdout(),
            Self::Stderr(_) => Self::stderr(),
            Self::DuplexStream(x) => Self::DuplexStream(x.clone()),
            Self::Sink(_) => Self::sink(),
        }
    }
}
