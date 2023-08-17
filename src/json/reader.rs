use std::marker::PhantomData;
use std::pin::Pin;

use miette::Context;
use miette::IntoDiagnostic;
use serde::de::DeserializeOwned;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncRead;
use tokio::io::BufReader;
use tokio::io::Lines;

/// A JSONL (newline-delimited JSON) reader from a stream `R`, deserializing values of type `T`.
pub struct JsonReader<T, R> {
    /// The underlying reader.
    reader: Pin<Box<Lines<BufReader<R>>>>,
    _phantom: PhantomData<T>,
}

impl<T, R> JsonReader<T, R>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    /// Create a new reader wrapping the underlying stream.
    pub fn new(reader: R) -> Self {
        Self {
            reader: Box::pin(BufReader::new(reader).lines()),
            _phantom: PhantomData,
        }
    }

    /// Deserialize the next line from the reader.
    pub async fn next(&mut self) -> miette::Result<Option<T>> {
        let line = match self
            .reader
            .next_line()
            .await
            .into_diagnostic()
            .wrap_err("Failed to read line")?
        {
            Some(line) => line,
            None => {
                return Ok(None);
            }
        };

        // TODO: Attach source code information here?
        let value = serde_json::from_str(&line)
            .into_diagnostic()
            .wrap_err("Failed to deserialize JSON")?;

        Ok(Some(value))
    }
}
