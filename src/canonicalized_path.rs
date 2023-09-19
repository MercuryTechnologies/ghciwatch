//! Wrapper type for canonicalized [`Utf8PathBuf`]s.

use std::borrow::Borrow;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Deref;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::Context;
use miette::IntoDiagnostic;

/// A canonicalized [`Utf8PathBuf`].
///
/// This stores both the canonicalized path and the pre-canonicalized path.
///
/// These paths are [`Display`]ed as the original path but compared ([`Hash`], [`Eq`], [`Ord`]) as
/// the canonicalized path.
#[derive(Debug, Clone)]
pub struct CanonicalizedUtf8PathBuf {
    canon: Utf8PathBuf,
    original: Utf8PathBuf,
}

impl CanonicalizedUtf8PathBuf {
    /// Get a reference to this canonical path, borrowed as a [`Utf8Path`].
    pub fn canon(&self) -> &Utf8Path {
        self.canon.as_path()
    }
}

// Hash, Eq, and Ord delegate to the canonical path.
impl Hash for CanonicalizedUtf8PathBuf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Hash::hash(&self.canon, state);
    }
}

impl PartialEq for CanonicalizedUtf8PathBuf {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.canon, &other.canon)
    }
}

impl Eq for CanonicalizedUtf8PathBuf {}

impl PartialOrd for CanonicalizedUtf8PathBuf {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        PartialOrd::partial_cmp(&self.canon, &other.canon)
    }
}

impl Ord for CanonicalizedUtf8PathBuf {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Ord::cmp(&self.canon, &other.canon)
    }
}

impl Display for CanonicalizedUtf8PathBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.original, f)
    }
}

impl TryFrom<&Utf8Path> for CanonicalizedUtf8PathBuf {
    type Error = miette::Report;

    fn try_from(value: &Utf8Path) -> Result<Self, Self::Error> {
        Ok(Self {
            canon: value
                .canonicalize_utf8()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to canonicalize path: {value}"))?,
            original: value.to_owned(),
        })
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
        value.canon
    }
}

impl AsRef<Utf8Path> for CanonicalizedUtf8PathBuf {
    fn as_ref(&self) -> &Utf8Path {
        &self.canon
    }
}

impl Borrow<Utf8Path> for CanonicalizedUtf8PathBuf {
    fn borrow(&self) -> &Utf8Path {
        self.canon.as_path()
    }
}

impl Deref for CanonicalizedUtf8PathBuf {
    type Target = Utf8PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.canon
    }
}
