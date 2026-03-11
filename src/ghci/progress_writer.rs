use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use crossterm::cursor::MoveToColumn;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use crossterm::QueueableCommand;
use tokio::io::AsyncWrite;
use winnow::Parser;

use super::writer::GhciWriter;
use crate::ghci::parse::compiling;

/// Equivalent to `s[..s.floor_char_boundary(max_cols)]` (stable in Rust 1.82).
fn truncate_to_terminal_width(s: &str, max_cols: usize) -> &str {
    if s.len() <= max_cols {
        return s;
    }
    let mut end = max_cols;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Wraps a [`GhciWriter`] and intercepts `[N of M] Compiling Module ...` lines,
/// rendering them as a single-line progress indicator instead of passing them through.
///
/// Non-progress lines (errors, warnings, etc.) are forwarded unchanged. When a non-progress
/// line arrives after progress output, the progress indicator is cleared first.
pub struct ProgressWriter {
    inner: GhciWriter,
    line_buffer: Vec<u8>,
    pending_output: VecDeque<u8>,
    progress_active: bool,
    render_progress: bool,
}

impl std::fmt::Debug for ProgressWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressWriter")
            .field("inner", &self.inner)
            .field("line_buffer_len", &self.line_buffer.len())
            .field("pending_output_len", &self.pending_output.len())
            .field("progress_active", &self.progress_active)
            .field("render_progress", &self.render_progress)
            .finish()
    }
}

impl ProgressWriter {
    pub fn new(inner: GhciWriter, render_progress: bool) -> Self {
        Self {
            inner,
            line_buffer: Vec::with_capacity(512),
            pending_output: VecDeque::new(),
            progress_active: false,
            render_progress,
        }
    }

    /// Create a fresh copy with the same configuration but empty buffers.
    /// Used when cloning a `GhciWriter` that wraps a `ProgressWriter` (e.g. on session restart).
    pub fn clone_fresh(&self) -> Self {
        Self::new(self.inner.clone(), self.render_progress)
    }

    /// Process complete lines in the line buffer, routing progress lines to the
    /// terminal and non-progress lines to `pending_output`.
    fn process_complete_lines(&mut self) {
        while let Some(newline_pos) = self.line_buffer.iter().position(|&b| b == b'\n') {
            let stripped = strip_ansi_escapes::strip_str(String::from_utf8_lossy(
                &self.line_buffer[..=newline_pos],
            ));

            if let Ok(progress) = compiling.parse(&stripped) {
                tracing::debug!(
                    current = progress.current,
                    total = progress.total,
                    module = %progress.module.name,
                    "Compilation progress",
                );

                self.render_progress(progress.current, progress.total, &progress.module.name);
            } else {
                if self.progress_active {
                    self.clear_progress();
                }
                self.pending_output
                    .extend(&self.line_buffer[..=newline_pos]);
            }

            self.line_buffer.drain(..=newline_pos);
        }
    }

    fn render_progress(&mut self, current: usize, total: usize, module: &str) {
        if !self.render_progress {
            return;
        }
        let line = format!("[{current}/{total}] Compiling {module}");
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        let truncated = truncate_to_terminal_width(&line, width.saturating_sub(1));
        let mut stdout = io::stdout();
        let _ = stdout.queue(MoveToColumn(0));
        let _ = stdout.queue(Clear(ClearType::CurrentLine));
        let _ = stdout.write_all(truncated.as_bytes());
        let _ = stdout.flush();
        self.progress_active = true;
    }

    fn clear_progress(&mut self) {
        if !self.render_progress || !self.progress_active {
            return;
        }
        let mut stdout = io::stdout();
        let _ = stdout.queue(MoveToColumn(0));
        let _ = stdout.queue(Clear(ClearType::CurrentLine));
        let _ = stdout.flush();
        self.progress_active = false;
    }

    /// Flush all bytes from `pending_output` into the inner writer.
    /// Returns `Poll::Ready(())` when fully flushed, `Poll::Pending` when the inner writer
    /// is not ready, or an error.
    fn flush_pending(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        while !self.pending_output.is_empty() {
            let result = {
                let (buf, _) = self.pending_output.as_slices();
                debug_assert!(!buf.is_empty());
                Pin::new(&mut self.inner).poll_write(cx, buf)
            };
            match result {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "inner writer returned 0 bytes written",
                    )));
                }
                Poll::Ready(Ok(n)) => {
                    self.pending_output.drain(..n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for ProgressWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = Pin::into_inner(self);

        // Flush any pending non-progress output from previous lines before accepting new input.
        match this.flush_pending(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }

        if !this.render_progress {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }

        this.line_buffer.extend_from_slice(buf);
        this.process_complete_lines();

        // Best-effort: unwritten data stays in pending_output for the next poll_write or poll_flush.
        let _ = this.flush_pending(cx);

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = Pin::into_inner(self);

        match this.flush_pending(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }

        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = Pin::into_inner(self);
        this.clear_progress();

        // Flush any remaining buffered content as-is (partial lines).
        if !this.line_buffer.is_empty() {
            this.pending_output.extend(this.line_buffer.drain(..));
        }

        match this.flush_pending(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }

        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn non_compiling_lines_pass_through() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), false);

        pw.write_all(b"some error output\n").await.unwrap();
        pw.flush().await.unwrap();

        let mut buf = vec![0u8; 4096];
        drop(pw);
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"some error output\n");
    }

    #[tokio::test]
    async fn compiling_lines_suppressed_on_tty() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 3] Compiling Foo ( Foo.hs, interpreted )\n")
            .await
            .unwrap();
        pw.write_all(b"some other output\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"some other output\n");
    }

    #[tokio::test]
    async fn render_disabled_passes_all_lines_through() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), false);

        pw.write_all(b"[1 of 3] Compiling Foo ( Foo.hs, interpreted )\n")
            .await
            .unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(
            &buf[..n],
            b"[1 of 3] Compiling Foo ( Foo.hs, interpreted )\n"
        );
    }

    #[tokio::test]
    async fn partial_line_buffering() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        // Line arrives in two chunks
        pw.write_all(b"[1 of 3] Compil").await.unwrap();
        pw.write_all(b"ing Foo ( Foo.hs, interpreted )\n")
            .await
            .unwrap();
        pw.write_all(b"error line\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"error line\n");
    }

    #[tokio::test]
    async fn multiple_progress_lines_suppressed() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 5] Compiling A ( A.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"[2 of 5] Compiling B ( B.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"[3 of 5] Compiling C ( C.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"Ok, 5 modules loaded.\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"Ok, 5 modules loaded.\n");
    }

    #[tokio::test]
    async fn padded_progress_numbers() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[   1 of 6508] Compiling A.Foo ( src/A/Foo.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"done\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"done\n");
    }

    #[tokio::test]
    async fn ansi_escapes_in_compiling_lines() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        // GHC may wrap progress lines in ANSI color codes
        pw.write_all(b"\x1b[0m[1 of 3] Compiling Foo ( Foo.hs, interpreted )\x1b[0m\n")
            .await
            .unwrap();
        pw.write_all(b"error output\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"error output\n");
    }

    #[tokio::test]
    async fn error_after_progress_passes_through() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 2] Compiling MyLib ( src/MyLib.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"[2 of 2] Compiling MyModule ( src/MyModule.hs )\n")
            .await
            .unwrap();
        let error_msg = b"\nsrc/MyModule.hs:4:11: error:\n    Type mismatch\n";
        pw.write_all(error_msg).await.unwrap();
        pw.write_all(b"Failed, one module loaded.\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        let output = std::str::from_utf8(&buf[..n]).unwrap();
        expect_test::expect![[r#"

            src/MyModule.hs:4:11: error:
                Type mismatch
            Failed, one module loaded.
        "#]]
        .assert_eq(output);
    }

    #[tokio::test]
    async fn error_between_progress_lines() {
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 3] Compiling A ( A.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"src/A.hs:1:1: warning: Missing signature\n")
            .await
            .unwrap();
        pw.write_all(b"[2 of 3] Compiling B ( B.hs )\n")
            .await
            .unwrap();
        pw.write_all(b"Ok, 3 modules loaded.\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        let output = std::str::from_utf8(&buf[..n]).unwrap();
        expect_test::expect![[r#"
            src/A.hs:1:1: warning: Missing signature
            Ok, 3 modules loaded.
        "#]]
        .assert_eq(output);
    }

    #[tokio::test]
    async fn incremental_reader_style_writes() {
        // Simulate how IncrementalReader writes: line content then \n separately
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 3] Compiling Foo ( Foo.hs )")
            .await
            .unwrap();
        pw.write_all(b"\n").await.unwrap();
        pw.write_all(b"other output").await.unwrap();
        pw.write_all(b"\n").await.unwrap();
        pw.flush().await.unwrap();
        drop(pw);

        let mut buf = vec![0u8; 4096];
        let mut reader = tokio::io::BufReader::new(reader);
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"other output\n");
    }
}
