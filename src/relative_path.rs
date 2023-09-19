//! Wrapper types for paths that display as relative paths.

use std::borrow::Borrow;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Deref;

use camino::Utf8Path;
use camino::Utf8PathBuf;

/// A relative [`Utf8PathBuf`].
///
/// This stores the relative path and is generic over the original path.
///
/// These paths are [`Display`]ed as the relative path but compared ([`Hash`], [`Eq`], [`Ord`]) as
/// the original path.
#[derive(Debug, Clone)]
pub struct RelativePath<P> {
    relative: Utf8PathBuf,
    original: P,
}

impl<P> RelativePath<P> {
    /// Get the relative path as a reference.
    pub fn relative(&self) -> &Utf8PathBuf {
        &self.relative
    }

    /// Get the original path as a reference.
    pub fn original(&self) -> &P {
        &self.original
    }
}

impl<P> RelativePath<P>
where
    P: AsRef<Utf8Path>,
{
    /// Create a new relative path by making the `original` path relative to `base`.
    ///
    /// If the `original` path cannot be made relative, it's used unchanged.
    pub fn new(original: P, base: impl AsRef<Utf8Path>) -> Self {
        let original_ref = original.as_ref();
        let relative = match pathdiff::diff_utf8_paths(original_ref, base) {
            Some(path) => path,
            None => original_ref.to_owned(),
        };
        Self { relative, original }
    }
}

impl<P: Hash> Hash for RelativePath<P> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Hash::hash(&self.original, state);
    }
}

impl<P: PartialEq> PartialEq for RelativePath<P> {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.original, &other.original)
    }
}

impl<P: Eq> Eq for RelativePath<P> {}

impl<P: PartialOrd> PartialOrd for RelativePath<P> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        PartialOrd::partial_cmp(&self.original, &other.original)
    }
}

impl<P: Ord> Ord for RelativePath<P> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Ord::cmp(&self.original, &other.original)
    }
}

impl<P> Display for RelativePath<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.relative, f)
    }
}

impl<P: AsRef<Utf8Path>> AsRef<Utf8Path> for RelativePath<P> {
    fn as_ref(&self) -> &Utf8Path {
        self.original.as_ref()
    }
}

impl<P: Borrow<Utf8Path>> Borrow<Utf8Path> for RelativePath<P> {
    fn borrow(&self) -> &Utf8Path {
        self.original.borrow()
    }
}

impl<P> Borrow<P> for RelativePath<P> {
    fn borrow(&self) -> &P {
        self.original.borrow()
    }
}

impl<P> Borrow<P> for &RelativePath<P> {
    fn borrow(&self) -> &P {
        self.original.borrow()
    }
}

impl<P, T> Deref for RelativePath<P>
where
    P: Deref<Target = T>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.original.deref()
    }
}
