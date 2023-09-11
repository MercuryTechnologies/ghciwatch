use std::collections::HashSet;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::Context;
use miette::IntoDiagnostic;

use super::Module;

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSet {
    set: HashSet<Utf8PathBuf>,
}

impl ModuleSet {
    /// Parse a `ModuleSet` from a set of lines read from `ghci` stdout.
    pub fn from_lines(lines: &str) -> miette::Result<Self> {
        Ok(Self {
            set: lines
                .lines()
                .map(|line| {
                    line.parse::<Module>()
                        .wrap_err("Failed to parse `:show modules` line")
                        .and_then(|module| canonicalize(&module.path))
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Get the number of modules in this set.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Determine if this set is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove all entries from this set, leaving it empty.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.set.clear();
    }

    /// Determine if a module with the given source path is contained in this module set.
    ///
    /// Returns `Err` if the `path` cannot be canonicalized.
    #[allow(dead_code)]
    pub fn contains_source_path(&self, path: &Utf8Path) -> miette::Result<bool> {
        Ok(self.set.contains(&canonicalize(path)?))
    }

    /// Add a source path to this module set.
    ///
    /// Returns `Err` if the `path` cannot be canonicalized.
    pub fn insert_source_path(&mut self, path: &Utf8Path) -> miette::Result<()> {
        self.set.insert(canonicalize(path)?);
        Ok(())
    }

    /// Remove a source path from this module set.
    ///
    /// Returns whether the path was present in the set.
    ///
    /// Returns `Err` if the `path` cannot be canonicalized.
    #[allow(dead_code)]
    pub fn remove_source_path(&mut self, path: &Utf8Path) -> miette::Result<bool> {
        Ok(self.set.remove(&canonicalize(path)?))
    }
}

/// Canonicalize the given path.
fn canonicalize(path: &Utf8Path) -> miette::Result<Utf8PathBuf> {
    path.canonicalize_utf8()
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to canonicalize path: {path:?}"))
}
