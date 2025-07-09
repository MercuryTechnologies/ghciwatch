use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use tokio::io::AsyncRead;
use tokio::io::ReadBuf;

/// A fake [`AsyncRead`] implementation for testing.
///
/// A `FakeReader` contains a number of "chunks" of bytes. When [`AsyncRead::poll_read`] is called,
/// the next chunk is delivered. This allows testers to supply all the data that can be read out of
/// this reader up front while simulating streaming conditions where not all of the data can be
/// read at once.
#[derive(Debug, Default, Clone)]
pub struct FakeReader {
    chunks: VecDeque<Vec<u8>>,
}

impl FakeReader {
    /// Construct a `FakeReader` from an iterator of strings.
    pub fn with_byte_chunks<const N: usize>(chunks: [&[u8]; N]) -> Self {
        Self {
            chunks: chunks.into_iter().map(|chunk| chunk.to_vec()).collect(),
        }
    }
}

impl AsyncRead for FakeReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.chunks.pop_front() {
            Some(mut chunk) => {
                let remaining = buf.remaining();
                if chunk.len() <= remaining {
                    // Write the whole chunk.
                    buf.put_slice(&chunk);
                } else {
                    // Write `remaining` bytes of the chunk, and reinsert the rest of it at the
                    // front of the deque.
                    let rest = chunk.split_off(remaining);
                    buf.put_slice(&chunk);
                    self.chunks.push_front(rest);
                }
                Poll::Ready(Ok(()))
            }
            None => {
                // Ok(()) without writing any data means EOF.
                Poll::Ready(Ok(()))
            }
        }
    }
}
