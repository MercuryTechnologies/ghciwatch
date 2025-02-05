//! The [`IncrementalReader`] struct, which handles reading to delimiters without line buffering.

use std::borrow::Cow;
use std::pin::Pin;

use aho_corasick::AhoCorasick;
use line_span::LineSpans;
use miette::IntoDiagnostic;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::aho_corasick::AhoCorasickExt;
use crate::buffers::LINE_BUFFER_CAPACITY;
use crate::buffers::SPLIT_UTF8_CODEPOINT_CAPACITY;
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
    /// The wrapped writer, if any.
    writer: Option<Pin<Box<W>>>,
    /// Lines we've already read since the last marker/chunk.
    lines: String,
    /// The line currently being written to.
    line: String,
    /// We're not guaranteed that the data we read at one time is aligned on a UTF-8 boundary. If
    /// that's the case, we store the data here until we get more data.
    non_utf8: Vec<u8>,
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
            writer: None,
            lines: String::with_capacity(VEC_BUFFER_CAPACITY * LINE_BUFFER_CAPACITY),
            line: String::with_capacity(LINE_BUFFER_CAPACITY),
            non_utf8: Vec::with_capacity(SPLIT_UTF8_CODEPOINT_CAPACITY),
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
    pub async fn read_until(&mut self, opts: &mut ReadOpts<'_>) -> miette::Result<String> {
        loop {
            if let Some(lines) = self.try_read_until(opts).await? {
                return Ok(lines);
            }
        }
    }

    /// Examines the internal buffer and reads at most once from the underlying reader. If a line
    /// beginning with one of the `end_marker` patterns is seen, the lines before the marker are
    /// returned. Otherwise, nothing is returned.
    pub async fn try_read_until(
        &mut self,
        opts: &mut ReadOpts<'_>,
    ) -> miette::Result<Option<String>> {
        if let Some(chunk) = self.take_chunk_from_buffer(opts) {
            tracing::trace!(data = chunk.len(), "Got data from buffer");
            return Ok(Some(chunk));
        }

        match self.reader.read(opts.buffer).await {
            Ok(0) => {
                // EOF
                Ok(None)
            }
            Ok(n) => {
                let decoded = self.decode(&opts.buffer[..n]);
                match self.consume_str(&decoded, opts).await? {
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

    fn decode(&mut self, buffer: &[u8]) -> String {
        // Do we have data we failed to decode?
        let buffer = if self.non_utf8.is_empty() {
            Cow::Borrowed(buffer)
        } else {
            // We have some data that failed to decode when we read it, add the data we just read
            // and hope that completes a UTF-8 boundary:
            let mut non_utf8 = std::mem::replace(
                &mut self.non_utf8,
                Vec::with_capacity(SPLIT_UTF8_CODEPOINT_CAPACITY),
            );
            non_utf8.extend(buffer);
            Cow::Owned(non_utf8)
        };

        match std::str::from_utf8(&buffer) {
            Ok(data) => data.to_owned(),
            Err(err) => {
                match err.error_len() {
                    Some(_) => {
                        // An unexpected byte was encountered; this is a "real" UTF-8 decode
                        // failure that we can't recover from by reading more data.
                        //
                        // As a backup, we'll log an error and decode the rest lossily.
                        tracing::error!("Failed to decode UTF-8 from `ghci`: {err}.\n\
                            This is a bug, please report it upstream: https://github.com/MercuryTechnologies/ghciwatch/issues/new");
                        String::from_utf8_lossy(&buffer).into_owned()
                    }
                    None => {
                        // End of input reached unexpectedly.
                        let valid_utf8 = &buffer[..err.valid_up_to()];
                        self.non_utf8.extend(&buffer[err.valid_up_to()..]);
                        unsafe {
                            // Safety: We already confirmed the input contains valid UTF-8 up to
                            // this index.
                            std::str::from_utf8_unchecked(valid_utf8).to_owned()
                        }
                    }
                }
            }
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
        opts: &ReadOpts<'_>,
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
                            // HACK(jadel): eat ANSI escapes right before doing our end
                            // marker matching. This is because hspec is a naughty
                            // library and outputs "\n\x1b[0mMARKER" which is really
                            // rude of it imo.
                            let stripped = strip_ansi_escapes::strip_str(&self.line);

                            match opts.find(opts.end_marker, &stripped) {
                                Some(_match) => {
                                    // If we found an `end_marker` in `self.line`, our chunk is
                                    // `self.lines`.
                                    Some(self.take_lines(opts.writing).await?)
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
                            self.finish_line(opts.writing).await?;
                            Some(lines)
                        }
                        None => {
                            // We don't have a chunk to return yet, so check for an `end_marker`.
                            match opts.find(opts.end_marker, &self.line) {
                                Some(_match) => {
                                    // If we found an `end_marker` in `self.line`, our chunk is
                                    // `self.lines`.
                                    Some(self.take_lines(opts.writing).await?)
                                }
                                None => {
                                    // We didn't find an `end_marker`, so add the current line to
                                    // `self.lines` and continue to process the remaining data in
                                    // `rest.
                                    self.finish_line(opts.writing).await?;
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
        tracing::debug!(line, "Read line");
        self.lines.push_str(&line);
        self.lines.push('\n');

        Ok(())
    }

    /// Examines the internal buffer. If a line beginning with one of the `end_marker` patterns is
    /// seen, the lines before the marker are returned. Otherwise, nothing is returned.
    ///
    /// Does _not_ read from the underlying reader.
    fn take_chunk_from_buffer(&mut self, opts: &ReadOpts<'_>) -> Option<String> {
        // Do any of the lines in `self.lines` start with `end_marker`?
        if let Some(span) = self
            .lines
            .line_spans()
            .find(|span| opts.find(opts.end_marker, span.as_str()).is_some())
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
        if opts.find(opts.end_marker, &self.line).is_some() {
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

/// Determines where an [`IncrementalReader`] matches an [`AhoCorasick`] end marker.
#[derive(Clone, Copy, Debug)]
pub enum FindAt {
    /// Match only at the start of a line.
    LineStart,
    /// Match anywhere in a line.
    Anywhere,
}

/// Options for performing a read from an [`IncrementalReader`].
#[derive(Debug)]
pub struct ReadOpts<'a> {
    /// The end marker to look for.
    pub end_marker: &'a AhoCorasick,
    /// Where the end marker should be looked for.
    pub find: FindAt,
    /// How to write output to the wrapped writer.
    pub writing: WriteBehavior,
    /// A buffer to read input into. This is used to avoid allocating additional buffers; no
    /// particular constraints are placed on this buffer.
    pub buffer: &'a mut [u8],
}

impl<'a> ReadOpts<'a> {
    fn find(&self, marker: &AhoCorasick, input: &str) -> Option<aho_corasick::Match> {
        match self.find {
            FindAt::LineStart => marker.find_at_start(input),
            FindAt::Anywhere => marker.find_anywhere(input),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::fake_reader::FakeReader;

    use super::*;

    /// Basic test. Reads data from the reader, gets the first chunk.
    #[tokio::test]
    async fn test_read_until() {
        let fake_reader = FakeReader::with_byte_chunks([indoc!(
            b"Build profile: -w ghc-9.6.1 -O0
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
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::LineStart,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer,
                })
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

    /// Same as `test_read_until` but with `FindAt::Anywhere`.
    #[tokio::test]
    async fn test_read_until_find_anywhere() {
        let fake_reader = FakeReader::with_byte_chunks([indoc!(
            b"Build profile: -w ghc-9.6.1 -O0
            In order, the following will be built (use -v for more details):
             - mwb-0 (lib:test-dev) (ephemeral targets)
            Preprocessing library 'test-dev' for mwb-0..
            GHCi, version 9.6.1: https://www.haskell.org/ghc/  :? for help
            Loaded GHCi configuration from .ghci-mwb
            Ok, 5699 modules loaded.
            ghci> "
        )]);

        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["https://www.haskell.org/ghc/"]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::Anywhere,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer,
                })
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
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::LineStart,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer
                })
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

    #[tokio::test]
    async fn test_read_until_with_data_in_buffer_find_anywhere() {
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

        let end_marker = AhoCorasick::from_anchored_patterns(["https://www.haskell.org/ghc/"]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::Anywhere,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer
                })
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
        let fake_reader = FakeReader::with_byte_chunks([
            b"Build profile: -w ghc-9.6.1 -O0\n",
            b"In order, the following will be built (use -v for more details):\n",
            b" - mwb-0 (lib:test-dev) (ephemeral targets)\n",
            b"Preprocessing library 'test-dev' for mwb-0..\n",
            b"GH",
            b"C",
            b"i",
            b",",
            b" ",
            b"v",
            b"e",
            b"r",
            b"s",
            b"i",
            b"o",
            b"n",
            b" ",
            b"9",
            b".6.1: https://www.haskell.org/ghc/  :? for help\n",
            b"Loaded GHCi configuration from .ghci-mwb",
            b"Ok, 5699 modules loaded.",
            b"ghci> ",
        ]);
        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["GHCi, version "]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::LineStart,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer
                })
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

    async fn step<T: AsyncRead, W: AsyncWrite>(
        reader: &mut IncrementalReader<T, W>,
        buffer: &mut Vec<u8>,
        end_marker: &AhoCorasick,
        expected: expect_test::Expect,
    ) {
        let out = reader
            .read_until(&mut ReadOpts {
                end_marker,
                find: FindAt::LineStart,
                writing: WriteBehavior::Hide,
                buffer,
            })
            .await
            .unwrap()
            .lines()
            // We escape here since expect_test seemingly only does raw strings
            // which should not have CR characters shoved in them in literal
            // form.
            .map(|l| l.trim_end().escape_debug().to_string())
            .join("\n");
        expected.assert_eq(&out);
    }

    /// Verifies that some kinda colour weirds from hspec don't break it.
    #[tokio::test]
    async fn try_read_until_colour_weirds() {
        let fake_reader = FakeReader::with_byte_chunks([
b"Build profile: -w ghc-9.6.6 -O1\n###~GHCIWATCH",
b"-PROMPT~###",
b"test/Main.hs",
b"\n###~GHCIWATCH-PROMPT~###",
b"###~GHCIWATCH-PROMPT~###",
b"\x1b[?25l\n\x1b[?7lhas unit [ ]\x1b[?7h",
b"\r\x1b[Khas unit [\x1b[32m\xE2\x9C\x94\x1b[0m]\n\nFinished in 0.0001 seconds\n\x1b[32m1 example, 0 failures\x1b[0m\n\x1b[?25h",
b"###~GHCIWATCH-PROMPT~###",
        ]);

        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["###~GHCIWATCH-PROMPT~###"]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect!["Build profile: -w ghc-9.6.6 -O1"],
        )
        .await;

        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect!["test/Main.hs"],
        )
        .await;
        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect![""],
        )
        .await;

        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect![[r#"
                \u{1b}[?25l
                \u{1b}[?7lhas unit [ ]\u{1b}[?7h\r\u{1b}[Khas unit [\u{1b}[32m✔\u{1b}[0m]

                Finished in 0.0001 seconds
                \u{1b}[32m1 example, 0 failures\u{1b}[0m"#]],
        )
        .await;

        assert_eq!(reader.buffer(), String::new());
    }

    #[tokio::test]
    async fn try_read_until_colour_weirds_ok() {
        let fake_reader = FakeReader::with_byte_chunks([
b"Build profile: -w ghc-9.6.6 -O1\n",
b"\xCE\xBB> ###~GHCIWATCH-PROMPT~",
b"###",
b"###~GHCIW",
b"ATCH-PROMPT~###",
b"current working direc",
b"tory: \n  /Users/jade/dev/repro/ghciwatch-bug-demo\nm",
b"odule import search paths:\n  test\n  dist-newstyle/build/aarch64-osx/ghc-9.6.6/ghciwatch-bug-demo-",
b"0.1.0.0/t/ghciwatch-bug-demo-test/build/ghciwatch-bug-demo-t",
b"est/ghciwatch-bug-demo-test-tmp\n  dist-newstyle/build/aarch64-",
b"osx/ghc-9.6.6/ghciwatch-bug-demo-0.1.0.0/t/ghciwatch-bug-demo-test/bu",
b"ild/ghciwatch-bug-demo-test/autogen\n  dist-newstyle/build/aarch64-os",
b"x/ghc-9.6.6/ghciwatch-bug-demo-0.1.0.0/t/ghciwatch-bug-demo-test/b",
b"uild/global-autogen\n###~GHCIWATCH-PROMPT~###",
b"test/Main",
b".hs\n###~GHCIWATCH-PROMPT~###",
b"###~GHCIWATCH-PROMPT~###",
b"\nhas unit [\xE2\x9C\x94]\n\n",
b"Finished in 0.0001 seconds\n1 example, 0 failures\n###~GHCIWATCH-PROMPT~###",
b"[1 of 2] Compiling Main             ( test/Main.hs, interpreted ) [Source file changed]\n",
b"Ok, one module loaded.\n",
b"###~GHCIWATCH-PROMPT~#",
b"##",
b"\nhas a unit [\xE2\x9C\x94]\n\n",
b"Finished in 0.0001 seconds\n1 example, 0 failures\n###~GHCIWATCH-PROMPT",
b"~###",
        ]);

        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["###~GHCIWATCH-PROMPT~###"]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        step(&mut reader, &mut buffer, &end_marker, expect_test::expect![[r#"
            Build profile: -w ghc-9.6.6 -O1
            λ> ###~GHCIWATCH-PROMPT~######~GHCIWATCH-PROMPT~###current working directory:
              /Users/jade/dev/repro/ghciwatch-bug-demo
            module import search paths:
              test
              dist-newstyle/build/aarch64-osx/ghc-9.6.6/ghciwatch-bug-demo-0.1.0.0/t/ghciwatch-bug-demo-test/build/ghciwatch-bug-demo-test/ghciwatch-bug-demo-test-tmp
              dist-newstyle/build/aarch64-osx/ghc-9.6.6/ghciwatch-bug-demo-0.1.0.0/t/ghciwatch-bug-demo-test/build/ghciwatch-bug-demo-test/autogen
              dist-newstyle/build/aarch64-osx/ghc-9.6.6/ghciwatch-bug-demo-0.1.0.0/t/ghciwatch-bug-demo-test/build/global-autogen"#]]).await;

        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect!["test/Main.hs"],
        )
        .await;
        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect![""],
        )
        .await;
        step(
            &mut reader,
            &mut buffer,
            &end_marker,
            expect_test::expect![[r#"

                has unit [✔]

                Finished in 0.0001 seconds
                1 example, 0 failures"#]],
        )
        .await;
    }

    /// Test that we can keep reading when a chunk from `read()` splits a UTF-8 boundary.
    async fn utf8_boundary<const N: usize>(chunks: [&'static [u8]; N], decoded: &'static str) {
        let fake_reader = FakeReader::with_byte_chunks(chunks);
        let mut reader = IncrementalReader::new(fake_reader).with_writer(tokio::io::sink());
        let end_marker = AhoCorasick::from_anchored_patterns(["ghci> "]);
        let mut buffer = vec![0; LINE_BUFFER_CAPACITY];

        assert_eq!(
            reader
                .read_until(&mut ReadOpts {
                    end_marker: &end_marker,
                    find: FindAt::LineStart,
                    writing: WriteBehavior::Hide,
                    buffer: &mut buffer
                })
                .await
                .unwrap(),
            decoded,
            "Failed to decode codepoint {decoded:?} when split across two chunks: {chunks:?}",
        );

        assert_eq!(reader.buffer(), String::new());
    }

    #[tokio::test]
    async fn test_read_utf8_boundary_u_00a9() {
        // U+00A9 ©
        // 2 bytes, 1 test case.
        utf8_boundary([b"\xc2", b"\xa9\nghci> "], "©\n").await;
    }

    #[tokio::test]
    async fn test_read_utf8_boundary_u_2194() {
        // U+2194 ↔
        // 3 bytes, 2 test cases.
        utf8_boundary([b"\xe2", b"\x86\x94\nghci> "], "↔\n").await;
        utf8_boundary([b"\xe2\x86", b"\x94\nghci> "], "↔\n").await;
    }

    #[tokio::test]
    async fn test_read_utf8_boundary_u_1f436() {
        // U+1F436 🐶
        // 4 bytes, 3 test cases.
        utf8_boundary([b"\xf0", b"\x9f\x90\xb6\nghci> "], "🐶\n").await;
        utf8_boundary([b"\xf0\x9f", b"\x90\xb6\nghci> "], "🐶\n").await;
        utf8_boundary([b"\xf0\x9f\x90", b"\xb6\nghci> "], "🐶\n").await;
    }

    #[tokio::test]
    async fn test_read_invalid_utf8_overlong() {
        // Overlong sequence, U+20AC € encoded as 4 bytes.
        // We get four U+FFFD � replacement characters out, one for each byte in the sequence.
        utf8_boundary([b"\xf0", b"\x82\x82\xac\nghci> "], "����\n").await;
        utf8_boundary([b"\xf0\x82", b"\x82\xac\nghci> "], "����\n").await;
        utf8_boundary([b"\xf0\x82\x82", b"\xac\nghci> "], "����\n").await;
    }

    #[tokio::test]
    async fn test_read_invalid_utf8_surrogate_pair_half() {
        // Half of a surrogate pair, invalid in UTF-8. (U+D800)
        utf8_boundary([b"\xed", b"\xa0\x80\nghci> "], "���\n").await;
        utf8_boundary([b"\xed\xa0", b"\x80\nghci> "], "���\n").await;
    }

    #[tokio::test]
    async fn test_read_invalid_utf8_unexpected_continuation() {
        // An unexpected continuation byte.
        utf8_boundary([b"\xa0\x80\nghci> "], "��\n").await;
        utf8_boundary([b"\xa0", b"\x80\nghci> "], "��\n").await;
    }

    #[tokio::test]
    async fn test_read_invalid_utf8_missing_continuation() {
        // Missing continuation byte.
        // Weirdly, these only come out as one replacement character, not the three we might
        // naïvely expect.
        utf8_boundary([b"\xf0", b"\x9f\x90\nghci> "], "�\n").await;
        utf8_boundary([b"\xf0\x9f", b"\x90\nghci> "], "�\n").await;
    }

    #[tokio::test]
    async fn test_read_invalid_utf8_invalid_byte() {
        // Invalid byte (no defined meaning in UTF-8).
        utf8_boundary([b"\xc0\nghci> "], "�\n").await;
        utf8_boundary([b"\xc1\nghci> "], "�\n").await;
        utf8_boundary([b"\xf5\nghci> "], "�\n").await;
        utf8_boundary([b"\xff\nghci> "], "�\n").await;
    }
}
