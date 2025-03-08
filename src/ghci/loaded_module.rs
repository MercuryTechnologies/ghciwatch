use std::borrow::Borrow;
use std::fmt::Display;
use std::hash::Hash;
use std::hash::Hasher;

use camino::Utf8Path;

use crate::normal_path::NormalPath;

/// Information about a module loaded into a `ghci` session.
///
/// Hashing and equality are determined by the module's path alone.
#[derive(Debug, Clone, Eq)]
pub struct LoadedModule {
    /// The module's source file, like `src/My/Cool/Module.hs`.
    path: NormalPath,

    /// The module's dotted name, like `My.Cool.Module`.
    ///
    /// This is present if and only if the module is loaded by name.
    ///
    /// Entries in `:show targets` can be one of two types: module paths or module names (with `.` in
    /// place of path separators). Due to a `ghci` bug, the module can only be referred to as whichever
    /// form it was originally added as (see below), so we use this to track how we refer to modules.
    ///
    /// See: <https://gitlab.haskell.org/ghc/ghc/-/issues/13254#note_525037>
    name: Option<String>,
}

impl LoadedModule {
    /// Create a new module, loaded by path.
    pub fn new(path: NormalPath) -> Self {
        Self { path, name: None }
    }

    /// Create a new module, loaded by name.
    pub fn with_name(path: NormalPath, name: String) -> Self {
        Self {
            path,
            name: Some(name),
        }
    }

    /// Get the module's source path.
    pub fn path(&self) -> &NormalPath {
        &self.path
    }
}

impl Display for LoadedModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.name
                .as_deref()
                .unwrap_or_else(|| self.path.relative().as_str())
        )
    }
}

impl Hash for LoadedModule {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state)
    }
}

impl PartialEq for LoadedModule {
    fn eq(&self, other: &Self) -> bool {
        self.path.eq(&other.path)
    }
}

impl PartialOrd for LoadedModule {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LoadedModule {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}

impl Borrow<NormalPath> for LoadedModule {
    fn borrow(&self) -> &NormalPath {
        &self.path
    }
}

impl Borrow<Utf8Path> for LoadedModule {
    fn borrow(&self) -> &Utf8Path {
        &self.path
    }
}
