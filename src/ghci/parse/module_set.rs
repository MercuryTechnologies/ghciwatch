use std::borrow::Borrow;
use std::cmp::Eq;
use std::collections::HashSet;
use std::hash::Hash;

use camino::Utf8Path;
use miette::Context;

use crate::canonicalized_path::CanonicalizedUtf8PathBuf;

use super::Module;

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSet {
    set: HashSet<CanonicalizedUtf8PathBuf>,
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
                        .and_then(|module| module.path.try_into())
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Get the number of modules in this set.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Determine if this set is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove all entries from this set, leaving it empty.
    pub fn clear(&mut self) {
        self.set.clear();
    }

    /// Determine if a module with the given source path is contained in this module set.
    pub fn contains_source_path<P>(&self, path: &P) -> miette::Result<bool>
    where
        CanonicalizedUtf8PathBuf: Borrow<P>,
        P: Hash + Eq,
    {
        Ok(self.set.contains(path))
    }

    /// Add a source path to this module set.
    pub fn insert_source_path(&mut self, path: CanonicalizedUtf8PathBuf) -> miette::Result<()> {
        self.set.insert(path);
        Ok(())
    }

    /// Remove a source path from this module set.
    ///
    /// Returns whether the path was present in the set.
    pub fn remove_source_path<P>(&mut self, path: &P)
    where
        CanonicalizedUtf8PathBuf: Borrow<P>,
        P: Hash + Eq,
    {
        self.set.remove(path);
    }
}
