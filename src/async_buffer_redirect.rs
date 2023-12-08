use gag::Redirect;
use std::fs::File as StdFile;
use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncRead;
use tokio::io::ReadBuf;

/// Buffer output in an in-memory buffer. Async version of [`gag::BufferRedirect`].
pub struct AsyncBufferRedirect {
    #[allow(dead_code)]
    redir: Redirect<StdFile>,
    outer: File,
}

/// An in-memory read-only buffer into which [`AsyncBufferRedirect`] buffers output. Async version
/// of [`gag::Buffer`].
pub struct AsyncBuffer(File);

impl AsyncRead for AsyncBuffer {
    #[inline(always)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut Pin::into_inner(self).0), cx, buf)
    }
}

impl AsyncBufferRedirect {
    /// Buffer stdout.
    #[allow(dead_code)]
    pub fn stdout() -> io::Result<Self> {
        let tempfile = NamedTempFile::new()?;
        let inner = tempfile.reopen()?;
        let outer = File::from_std(tempfile.reopen()?);
        let redir = Redirect::stdout(inner)?;
        Ok(Self { redir, outer })
    }

    /// Buffer stderr.
    pub fn stderr() -> io::Result<Self> {
        let tempfile = NamedTempFile::new()?;
        let inner = tempfile.reopen()?;
        let outer = File::from_std(tempfile.reopen()?);
        let redir = Redirect::stderr(inner)?;
        Ok(Self { redir, outer })
    }

    /// Extract the inner buffer and stop redirecting output.
    #[allow(dead_code)]
    pub fn into_inner(self) -> AsyncBuffer {
        AsyncBuffer(self.outer)
    }
}

impl AsyncRead for AsyncBufferRedirect {
    #[inline(always)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut Pin::into_inner(self).outer), cx, buf)
    }
}
