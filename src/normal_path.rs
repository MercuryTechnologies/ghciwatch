//! Wrapper type for normalized [`Utf8PathBuf`]s.

use std::borrow::Borrow;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Deref;
use std::path::Path;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use clap::builder::PathBufValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use path_absolutize::Absolutize;

/// A normalized [`Utf8PathBuf`] in tandem with a relative path.
///
/// Normalized paths are absolute paths with dots removed; see [`path_dedot`][path_dedot] and
/// [`path_absolutize`] for more details.
///
/// These paths are [`Display`]ed as the relative path but compared ([`Hash`], [`Eq`], [`Ord`]) as
/// the normalized path.
///
/// [path_dedot]: https://docs.rs/path-dedot/latest/path_dedot/
#[derive(Debug, Clone)]
pub struct NormalPath {
    normal: Utf8PathBuf,
    relative: Option<Utf8PathBuf>,
}

impl NormalPath {
    /// Creates a new normalized path relative to the given base path.
    pub fn new(original: impl AsRef<Path>, base: impl AsRef<Path>) -> miette::Result<Self> {
        let base = base.as_ref();
        let normal = original.as_ref().absolutize_from(base).into_diagnostic()?;
        let normal = normal
            .into_owned()
            .try_into()
            .map_err(|err| miette!("{err}"))?;
        let relative = match pathdiff::diff_paths(&normal, base) {
            Some(path) => Some(path.try_into().map_err(|err| miette!("{err}"))?),
            None => None,
        };
        Ok(Self { normal, relative })
    }

    /// Create a new normalized path relative to the current working directory.
    pub fn from_cwd(original: impl AsRef<Path>) -> miette::Result<Self> {
        Self::new(
            original,
            std::env::current_dir()
                .into_diagnostic()
                .wrap_err("Failed to get current directory")?,
        )
    }

    /// Get a reference to the absolute (normalized) path, borrowed as a [`Utf8Path`].
    pub fn absolute(&self) -> &Utf8Path {
        self.normal.as_path()
    }

    /// Get a reference to the relative path, borrowed as a [`Utf8Path`].
    ///
    /// If no relative path is present, the absolute (normalized) path is used instead.
    pub fn relative(&self) -> &Utf8Path {
        self.relative.as_deref().unwrap_or_else(|| self.absolute())
    }
}

// Hash, Eq, and Ord delegate to the normalized path.
impl Hash for NormalPath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Hash::hash(&self.normal, state);
    }
}

impl PartialEq for NormalPath {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.normal, &other.normal)
    }
}

impl Eq for NormalPath {}

impl PartialOrd for NormalPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NormalPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Ord::cmp(&self.normal, &other.normal)
    }
}

impl Display for NormalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.relative {
            Some(path) => Display::fmt(&path, f),
            None => Display::fmt(&self.normal, f),
        }
    }
}

impl From<NormalPath> for Utf8PathBuf {
    fn from(value: NormalPath) -> Self {
        value.normal
    }
}

impl AsRef<Utf8Path> for NormalPath {
    fn as_ref(&self) -> &Utf8Path {
        &self.normal
    }
}

impl AsRef<Path> for NormalPath {
    fn as_ref(&self) -> &Path {
        self.normal.as_std_path()
    }
}

impl Borrow<Utf8PathBuf> for NormalPath {
    fn borrow(&self) -> &Utf8PathBuf {
        &self.normal
    }
}

impl Borrow<Utf8Path> for NormalPath {
    fn borrow(&self) -> &Utf8Path {
        self.normal.as_path()
    }
}

impl Deref for NormalPath {
    type Target = Utf8PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.normal
    }
}

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
                crate::clap::value_validation_error(
                    arg,
                    &value.to_string_lossy(),
                    format!("{err:?}"),
                )
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
