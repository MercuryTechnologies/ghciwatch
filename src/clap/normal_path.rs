//! Adapter for parsing [`NormalPath`]s with a [`clap::builder::Arg::value_parser`].

use clap::builder::PathBufValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;

use crate::normal_path::NormalPath;

/// [`clap`] parser for [`NormalPath`] values.
#[derive(Default, Clone)]
pub struct NormalPathValueParser {
    inner: PathBufValueParser,
}

impl TypedValueParser for NormalPathValueParser {
    type Value = NormalPath;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        self.inner.parse_ref(cmd, arg, value).and_then(|path_buf| {
            NormalPath::from_cwd(path_buf).map_err(|err| {
                super::value_validation_error(arg, &value.to_string_lossy(), format!("{err:?}"))
            })
        })
    }
}

impl ValueParserFactory for NormalPath {
    type Parser = NormalPathValueParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}
