use std::io;
use std::io::Write;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use regex::Regex;
use tokio::io::AsyncWrite;

use super::writer::GhciWriter;

/// Wraps a [`GhciWriter`] and intercepts `[N of M] Compiling Module ...` lines,
/// rendering them as a single-line progress indicator instead of passing them through.
///
/// Non-progress lines (errors, warnings, etc.) are forwarded unchanged. When a non-progress
/// line arrives after progress output, the progress indicator is cleared first.
pub struct ProgressWriter {
    inner: GhciWriter,
    line_buffer: Vec<u8>,
    pending_output: Vec<u8>,
    progress_active: bool,
    is_tty: bool,
    progress_pattern: Regex,
}

impl std::fmt::Debug for ProgressWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressWriter")
            .field("inner", &self.inner)
            .field("line_buffer_len", &self.line_buffer.len())
            .field("pending_output_len", &self.pending_output.len())
            .field("progress_active", &self.progress_active)
            .field("is_tty", &self.is_tty)
            .finish()
    }
}

impl ProgressWriter {
    pub fn new(inner: GhciWriter, is_tty: bool) -> Self {
        Self {
            inner,
            line_buffer: Vec::with_capacity(512),
            pending_output: Vec::new(),
            progress_active: false,
            is_tty,
            // See also: the winnow parser in ghci/parse/ghc_message/compiling.rs
            progress_pattern: Regex::new(
                r"^\[?[ \t]*([0-9]+)[ \t]+of[ \t]+([0-9]+)\][ \t]+Compiling[ \t]+([^ \t]+)",
            )
            .expect("progress regex is valid"),
        }
    }

    /// Create a fresh copy with the same configuration but empty buffers.
    /// Used when cloning a `GhciWriter` that wraps a `ProgressWriter` (e.g. on session restart).
    pub fn clone_fresh(&self) -> Self {
        Self::new(self.inner.clone(), self.is_tty)
    }

    /// Process complete lines in the line buffer, routing progress lines to the
    /// terminal and non-progress lines to `pending_output`.
    fn process_complete_lines(&mut self) {
        loop {
            let newline_pos = match self.line_buffer.iter().position(|&b| b == b'\n') {
                Some(pos) => pos,
                None => break,
            };

            let stripped = strip_ansi_escapes::strip_str(
                &String::from_utf8_lossy(&self.line_buffer[..=newline_pos]),
            );

            if let Some(caps) = self.progress_pattern.captures(&stripped) {
                let current = &caps[1];
                let total = &caps[2];
                let module = &caps[3];

                tracing::debug!(
                    current = current,
                    total = total,
                    module = module,
                    "Compilation progress",
                );

                self.render_progress(current, total, module);
            } else {
                if self.progress_active {
                    self.clear_progress();
                }
                self.pending_output
                    .extend_from_slice(&self.line_buffer[..=newline_pos]);
            }

            self.line_buffer.drain(..=newline_pos);
        }
    }

    fn render_progress(&mut self, current: &str, total: &str, module: &str) {
        if !self.is_tty {
            return;
        }
        let line = format!("[{current}/{total}] Compiling {module}");
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        let max_len = width.saturating_sub(1);
        let truncated = if line.len() > max_len {
            let end = line[..max_len]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            &line[..end]
        } else {
            &line
        };
        let _ = io::stdout().write_all(b"\r\x1b[2K");
        let _ = io::stdout().write_all(truncated.as_bytes());
        let _ = io::stdout().flush();
        self.progress_active = true;
    }

    fn clear_progress(&mut self) {
        if !self.is_tty || !self.progress_active {
            return;
        }
        let _ = io::stdout().write_all(b"\r\x1b[2K");
        let _ = io::stdout().flush();
        self.progress_active = false;
    }

    /// Flush all bytes from `pending_output` into the inner writer.
    /// Returns `Poll::Ready(())` when fully flushed, `Poll::Pending` when the inner writer
    /// is not ready, or an error.
    fn flush_pending(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        while !self.pending_output.is_empty() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.pending_output) {
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

        if !this.is_tty {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }

        this.line_buffer.extend_from_slice(buf);
        this.process_complete_lines();

        // Best-effort flush of any newly generated pending output.
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
            this.pending_output.append(&mut this.line_buffer);
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
    async fn compiling_lines_pass_through_on_non_tty() {
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
        let output = &buf[..n];
        let output_str = std::str::from_utf8(output).unwrap();
        assert!(
            output_str.contains("src/MyModule.hs:4:11: error:"),
            "Error line should pass through, got: {output_str}"
        );
        assert!(
            output_str.contains("Failed, one module loaded."),
            "Summary should pass through, got: {output_str}"
        );
        assert!(
            !output_str.contains("[1 of 2]"),
            "Progress lines should not pass through, got: {output_str}"
        );
    }

    #[tokio::test]
    async fn incremental_reader_style_writes() {
        // Simulate how IncrementalReader writes: line content then \n separately
        let (reader, writer) = tokio::io::duplex(4096);
        let mut pw = ProgressWriter::new(GhciWriter::duplex_stream(writer), true);

        pw.write_all(b"[1 of 3] Compiling Foo ( Foo.hs )").await.unwrap();
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
