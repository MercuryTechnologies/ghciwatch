//! Adapter for parsing [`camino::Utf8PathBuf`] with a [`clap::builder::Arg::value_parser`].

use camino::Utf8PathBuf;
use clap::builder::PathBufValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;

#[derive(Default, Clone)]
struct Utf8PathBufValueParser {
    inner: PathBufValueParser,
}

impl TypedValueParser for Utf8PathBufValueParser {
    type Value = Utf8PathBuf;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        self.inner.parse_ref(cmd, arg, value).and_then(|path_buf| {
            Utf8PathBuf::from_path_buf(path_buf).map_err(|path_buf| {
                clap::Error::raw(
                    clap::error::ErrorKind::InvalidUtf8,
                    format!("Path isn't UTF-8: {path_buf:?}"),
                )
                .with_cmd(cmd)
            })
        })
    }
}

struct Utf8PathBufValueParserFactory;

impl ValueParserFactory for Utf8PathBufValueParserFactory {
    type Parser = Utf8PathBufValueParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}
