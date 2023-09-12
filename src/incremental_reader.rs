//! The [`IncrementalReader`] struct, which handles reading to delimiters without line buffering.

use std::pin::Pin;

use aho_corasick::AhoCorasick;
use line_span::LineSpans;
use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::aho_corasick::AhoCorasickExt;
use crate::buffers::LINE_BUFFER_CAPACITY;
use crate::buffers::VEC_BUFFER_CAPACITY;

/// A tool for incrementally reading from a stream like stdout (and forwarding that stream to a
/// writer).
///
/// `ghci` will print output on stdout, and then, when it's done, it will print `ghci> ` (or
/// whatever the prompt is) with no trailing newline, and then wait for user input before
/// continuing.
///
/// This means that we can't use simple line-buffered reading to process its output -- we'll never
/// see a newline after the prompt, and we'll wait forever.
///
/// This type reads output from its contained reader incrementally, separating it into lines. Once
/// it reaches a line beginning with a user-supplied end-marker, it will return the chunk of lines
/// before that marker.
pub struct IncrementalReader<R, W> {
    /// The wrapped reader.
    reader: Pin<Box<R>>,
    /// Lines we've already read since the last marker/chunk.
    lines: String,
    /// The line currently being written to.
    line: String,
    /// The wrapped writer, if any.
    writer: Option<Pin<Box<W>>>,
}

impl<R, W> IncrementalReader<R, W>
where
    R: AsyncRead,
    W: AsyncWrite,
{
    /// Construct a new incremental reader wrapping the given reader.
    pub fn new(reader: R) -> Self {
        Self {
            reader: Box::pin(reader),
            lines: String::with_capacity(VEC_BUFFER_CAPACITY * LINE_BUFFER_CAPACITY),
            line: String::with_capacity(LINE_BUFFER_CAPACITY),
            writer: None,
        }
    }

    /// Add a writer to this reader. This will forward all output that this object reads to the
    /// given writer.
    pub fn with_writer(self, writer: W) -> Self {
        Self {
            writer: Some(Box::pin(writer)),
            ..self
        }
    }

    /// Read from the contained reader until a line beginning with one of the `end_marker` patterns
    /// is seen, returning the lines until the marker is found.
    ///
    /// This calls [`Self::try_read_until`] in a loop.
    ///
    /// TODO: Should this even use `aho_corasick`? Might be overkill, and with the automaton
    /// construction cost it might not even be more efficient.
    pub async fn read_until(
        &mut self,
        end_marker: &AhoCorasick,
        writing: WriteBehavior,
        buffer: &mut [u8],
    ) -> miette::Result<String> {
        loop {
            if let Some(lines) = self.try_read_until(end_marker, writing, buffer).await? {
                return Ok(lines);
            }
        }
    }

    /// Examines the internal buffer and reads at most once from the underlying reader. If a line
    /// beginning with one of the `end_marker` patterns is seen, the lines before the marker are
    /// returned. Otherwise, nothing is returned.
    pub async fn try_read_until(
        &mut self,
        end_marker: &AhoCorasick,
        writing: WriteBehavior,
        buffer: &mut [u8],
    ) -> miette::Result<Option<String>> {
        if let Some(chunk) = self.take_chunk_from_buffer(end_marker) {
            tracing::trace!(data = chunk.len(), "Got data from buffer");
            return Ok(Some(chunk));
        }

        match self.reader.read(buffer).await {
            Ok(0) => {
                // EOF
                Err(miette!("End-of-file reached"))
            }
            Ok(n) => {
                let decoded = std::str::from_utf8(&buffer[..n])
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!(
                            "Read invalid UTF-8: {:?}",
                            String::from_utf8_lossy(&buffer[..n])
                        )
                    })?;
                match self.consume_str(decoded, end_marker, writing).await? {
                    Some(lines) => {
                        tracing::trace!(data = decoded, "Decoded data");
                        tracing::trace!(lines = lines.len(), "Got chunk");
                        Ok(Some(lines))
                    }
                    None => {
                        tracing::trace!(data = decoded, "Decoded data, no end marker found");
                        Ok(None)
                    }
                }
            }
            Err(err) => Err(err).into_diagnostic(),
        }
    }

    /// Consumes a string, adding it to the internal buffer.
    ///
    /// If one of the lines in `data` begins with `end_marker`, the lines in the internal buffer
    /// until that line are returned, and the rest of the data is left in the internal buffer for
    /// future reading.
    ///
    /// Otherwise, nothing is returned.
    async fn consume_str(
        &mut self,
        mut data: &str,
        end_marker: &AhoCorasick,
        writing: WriteBehavior,
    ) -> miette::Result<Option<String>> {
        // Proof of this function's corectness: just trust me

        let mut ret = None;

        loop {
            match data.split_once('\n') {
                None => {
                    // No newline, just append to the current line.
                    self.line.push_str(data);

                    // If we don't have a chunk to return yet, check for an `end_marker`.
                    ret = match ret {
                        Some(lines) => Some(lines),
                        None => {
                            match end_marker.find_at_start(&self.line) {
                                Some(_match) => {
                                    // If we found an `end_marker` in `self.line`, our chunk is
                                    // `self.lines`.
                                    Some(self.take_lines(writing).await?)
                                }
                                None => None,
                            }
                        }
                    };

                    // No more data, so we're done.
                    break;
                }

                Some((first_line, rest)) => {
                    // Add the rest of the first line to the current line.
                    self.line.push_str(first_line);

                    ret = match ret {
                        Some(lines) => {
                            // We already have a chunk to return, so we can just add the current
                            // line to `self.lines` and continue to process the remaining data in
                            // `rest`.
                            self.finish_line(writing).await?;
                            Some(lines)
                        }
                        None => {
                            // We don't have a chunk to return yet, so check for an `end_marker`.
                            match end_marker.find_at_start(&self.line) {
                                Some(_match) => {
                                    // If we found an `end_marker` in `self.line`, our chunk is
                                    // `self.lines`.
                                    Some(self.take_lines(writing).await?)
                                }
                                None => {
                                    // We didn't find an `end_marker`, so add the current line to
                                    // `self.lines` and continue to process the remaining data in
                                    // `rest.
                                    self.finish_line(writing).await?;
                                    None
                                }
                            }
                        }
                    };

                    // Continue processing the rest of the data.
                    data = rest;
                }
            }
        }

        Ok(ret)
    }

    /// Clears `self.lines` and `self.line`, returning the previous value of `self.lines`.
    async fn take_lines(&mut self, writing: WriteBehavior) -> miette::Result<String> {
        if let Some(writer) = &mut self.writer {
            match writing {
                WriteBehavior::Write => {
                    writer
                        .write_all(self.line.as_bytes())
                        .await
                        .into_diagnostic()?;
                    // We'll just pretend this is the end of the line...
                    writer.write_all(b"\n").await.into_diagnostic()?;
                }
                WriteBehavior::NoFinalLine | WriteBehavior::Hide => {}
            }
        }

        self.line.clear();
        Ok(std::mem::replace(
            &mut self.lines,
            String::with_capacity(VEC_BUFFER_CAPACITY * LINE_BUFFER_CAPACITY),
        ))
    }

    /// Add `self.line` to `self.lines`, replacing `self.line` with an empty buffer.
    async fn finish_line(&mut self, writing: WriteBehavior) -> miette::Result<()> {
        if let Some(writer) = &mut self.writer {
            match writing {
                WriteBehavior::Write | WriteBehavior::NoFinalLine => {
                    writer
                        .write_all(self.line.as_bytes())
                        .await
                        .into_diagnostic()?;
                    writer.write_all(b"\n").await.into_diagnostic()?;
                }
                WriteBehavior::Hide => {}
            }
        }

        let line = std::mem::replace(&mut self.line, String::with_capacity(LINE_BUFFER_CAPACITY));
        tracing::trace!(line, "Read line");
        self.lines.push_str(&line);
        self.lines.push('\n');

        Ok(())
    }

    /// Examines the internal buffer. If a line beginning with one of the `end_marker` patterns is
    /// seen, the lines before the marker are returned. Otherwise, nothing is returned.
    ///
    /// Does _not_ read from the underlying reader.
    fn take_chunk_from_buffer(&mut self, end_marker: &AhoCorasick) -> Option<String> {
        // Do any of the lines in `self.lines` start with `end_marker`?
        if let Some(span) = self
            .lines
            .line_spans()
            .find(|span| end_marker.find_at_start(span.as_str()).is_some())
        {
            // Suppose this is our original `self.lines`, with newlines indicated by `|`:
            //
            // -----------|--------------|--------------|------------|
            //             ^^^^^^^^^^^^^^^
            //             `span`
            let range = span.range_with_ending();
            let rest = self.lines.split_off(range.start);
            // Now, we have:
            // -----------|--------------|--------------|------------|
            // ^^^^^^^^^^^^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
            // `self.lines`            `rest`
            //      ↓                    ↓
            //   `chunk`            `self.lines`
            let chunk = std::mem::replace(&mut self.lines, rest);
            // Finally, we remove the line we matched from `self.lines`:
            // -----------|--------------|--------------|------------|
            // ^^^^^^^^^^^^               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            //   `chunk`                           `self.lines`
            self.lines = self.lines.split_off(range.end - range.start);
            return Some(chunk);
        }

        // Does the current line in `self.line` start with `end_marker`?
        if end_marker.find_at_start(&self.line).is_some() {
            let chunk = std::mem::replace(
                &mut self.lines,
                String::with_capacity(VEC_BUFFER_CAPACITY * LINE_BUFFER_CAPACITY),
            );
            self.line.clear();
            return Some(chunk);
        }

        None
    }

    /// Add arbitrary data to the internal buffer. Used for testing.
    #[cfg(test)]
    async fn push_to_buffer(&mut self, data: &str) {
        for span in data.line_spans() {
            self.line.push_str(span.as_str());
            if !span.ending_str().is_empty() {
                self.finish_line(WriteBehavior::Write).await.unwrap();
            }
        }
    }

    /// Get the internal buffer as a list of lines. Used for testing.
    #[cfg(test)]
    fn buffer(&self) -> String {
        let mut ret = self.lines.clone();
        if !self.line.is_empty() {
            ret.push_str(&self.line);
        }
        ret
    }
}

/// Determines how an [`IncrementalReader`] forwards output to its contained writer. See
/// [`IncrementalReader::read_until`].
#[derive(Clone, Copy, Debug)]
pub enum WriteBehavior {
    /// Write all data, including the final line.
    Write,
    /// Write all data up until the final line.
    NoFinalLine,
    /// Hide all data.
    Hide,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    use crate::fake_reader::FakeReader;

    use super::*;

    /// Basic test. Reads data from the reader, gets the first chunk.
    #[tokio::test]
    async fn test_read_until() {
        let fake_reader = FakeReader::with_str_chunks([indoc!(
            "Build profile: -w ghc-9.6.1 -O0
            In order, the following will be built (use -v for more details):
             - mwb-0 (lib:test-dev) (ephemeral targets)
            Preprocessing library 'test-dev' for mwb-0..
            GHCi, version 9.6.1: https://www.haskell.org/ghc/  :? for help
            Loaded GHCi configuration from .ghci-mwb
            Ok, 5699 modules loaded.
            ghci> "
        )]);

        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["GHCi, version "]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&end_marker, WriteBehavior::Hide, &mut buffer)
                .await
                .unwrap(),
            indoc!(
                "
                Build profile: -w ghc-9.6.1 -O0
                In order, the following will be built (use -v for more details):
                 - mwb-0 (lib:test-dev) (ephemeral targets)
                Preprocessing library 'test-dev' for mwb-0..
                "
            )
        );
    }

    /// Test that an `IncrementalReader` can read until an `end_marker` while only operating on
    /// data remaining in its internal buffer.
    #[tokio::test]
    async fn test_read_until_with_data_in_buffer() {
        let fake_reader = FakeReader::default();
        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        reader
            .push_to_buffer(indoc!(
                "
                Build profile: -w ghc-9.6.1 -O0
                In order, the following will be built (use -v for more details):
                 - mwb-0 (lib:test-dev) (ephemeral targets)
                Preprocessing library 'test-dev' for mwb-0..
                GHCi, version 9.6.1: https://www.haskell.org/ghc/  :? for help
                Loaded GHCi configuration from .ghci-mwb
                Ok, 5699 modules loaded.
                ghci> "
            ))
            .await;

        let end_marker = AhoCorasick::from_anchored_patterns(["GHCi, version "]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&end_marker, WriteBehavior::Hide, &mut buffer)
                .await
                .unwrap(),
            indoc!(
                "
                Build profile: -w ghc-9.6.1 -O0
                In order, the following will be built (use -v for more details):
                 - mwb-0 (lib:test-dev) (ephemeral targets)
                Preprocessing library 'test-dev' for mwb-0..
                "
            )
        );

        eprintln!("{:?}", reader.buffer());
        assert_eq!(
            reader.buffer(),
            indoc!(
                "Loaded GHCi configuration from .ghci-mwb
                Ok, 5699 modules loaded.
                ghci> "
            )
        );
    }

    /// Test that an `IncrementalReader` can read until an `end_marker` while reading in small
    /// chunks.
    #[tokio::test]
    async fn test_read_until_incremental() {
        let fake_reader = FakeReader::with_str_chunks([
            "Build profile: -w ghc-9.6.1 -O0\n",
            "In order, the following will be built (use -v for more details):\n",
            " - mwb-0 (lib:test-dev) (ephemeral targets)\n",
            "Preprocessing library 'test-dev' for mwb-0..\n",
            "GH",
            "C",
            "i",
            ",",
            " ",
            "v",
            "e",
            "r",
            "s",
            "i",
            "o",
            "n",
            " ",
            "9",
            ".6.1: https://www.haskell.org/ghc/  :? for help\n",
            "Loaded GHCi configuration from .ghci-mwb",
            "Ok, 5699 modules loaded.",
            "ghci> ",
        ]);
        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["GHCi, version "]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&end_marker, WriteBehavior::Hide, &mut buffer)
                .await
                .unwrap(),
            indoc!(
                "
                Build profile: -w ghc-9.6.1 -O0
                In order, the following will be built (use -v for more details):
                 - mwb-0 (lib:test-dev) (ephemeral targets)
                Preprocessing library 'test-dev' for mwb-0..
                "
            )
        );

        assert_eq!(reader.buffer(), String::new());
    }
}
