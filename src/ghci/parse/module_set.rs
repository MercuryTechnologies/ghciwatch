use std::borrow::Borrow;
use std::cmp::Eq;
use std::collections::hash_set::Iter;
use std::collections::HashSet;
use std::hash::Hash;
use std::path::Path;

use crate::normal_path::NormalPath;

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSet {
    set: HashSet<NormalPath>,
}

impl ModuleSet {
    /// Construct a `ModuleSet` from an iterator of module source paths.
    pub fn from_paths(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        current_dir: impl AsRef<Path>,
    ) -> miette::Result<Self> {
        let current_dir = current_dir.as_ref();
        Ok(Self {
            set: paths
                .into_iter()
                .map(|path| NormalPath::new(path.as_ref(), current_dir))
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
        NormalPath: Borrow<P>,
        P: Hash + Eq + ?Sized,
    {
        Ok(self.set.contains(path))
    }

    /// Add a source path to this module set.
    pub fn insert_source_path(&mut self, path: NormalPath) -> miette::Result<()> {
        self.set.insert(path);
        Ok(())
    }

    /// Remove a source path from this module set.
    ///
    /// Returns whether the path was present in the set.
    pub fn remove_source_path<P>(&mut self, path: &P)
    where
        NormalPath: Borrow<P>,
        P: Hash + Eq,
    {
        self.set.remove(path);
    }

    /// Iterate over the source paths in this module set.
    pub fn iter(&self) -> Iter<'_, NormalPath> {
        self.set.iter()
    }
}
