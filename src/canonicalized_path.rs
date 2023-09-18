//! Wrapper type for canonicalized [`Utf8PathBuf`]s.

use std::borrow::Borrow;
use std::fmt::Debug;
use std::fmt::Display;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::Context;
use miette::IntoDiagnostic;

/// A canonicalized [`Utf8PathBuf`].
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CanonicalizedUtf8PathBuf(Utf8PathBuf);

impl CanonicalizedUtf8PathBuf {
    /// Consume this path, producing the wrapped path buffer.
    pub fn into_path(self) -> Utf8PathBuf {
        self.0
    }

    /// Get a reference to this path, borrowed as a [`Utf8Path`].
    pub fn as_path(&self) -> &Utf8Path {
        self.0.as_path()
    }
}

impl Display for CanonicalizedUtf8PathBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Debug for CanonicalizedUtf8PathBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl TryFrom<&Utf8Path> for CanonicalizedUtf8PathBuf {
    type Error = miette::Report;

    fn try_from(value: &Utf8Path) -> Result<Self, Self::Error> {
        Ok(Self(
            value
                .canonicalize_utf8()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to canonicalize path: {value}"))?,
        ))
    }
}

impl TryFrom<Utf8PathBuf> for CanonicalizedUtf8PathBuf {
    type Error = miette::Report;

    fn try_from(value: Utf8PathBuf) -> Result<Self, Self::Error> {
        value.as_path().try_into()
    }
}

impl From<CanonicalizedUtf8PathBuf> for Utf8PathBuf {
    fn from(value: CanonicalizedUtf8PathBuf) -> Self {
        value.into_path()
    }
}

impl AsRef<Utf8Path> for CanonicalizedUtf8PathBuf {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

impl Borrow<Utf8Path> for CanonicalizedUtf8PathBuf {
    fn borrow(&self) -> &Utf8Path {
        self.0.as_path()
    }
}
