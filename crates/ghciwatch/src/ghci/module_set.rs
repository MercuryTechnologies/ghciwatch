use std::borrow::Borrow;
use std::borrow::Cow;
use std::cmp::Eq;
use std::collections::HashSet;
use std::hash::Hash;

use crate::normal_path::NormalPath;

use super::loaded_module::LoadedModule;

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSet {
    modules: HashSet<LoadedModule>,
}

impl ModuleSet {
    /// Iterate over the modules in this set.
    pub fn iter(&self) -> std::collections::hash_set::Iter<'_, LoadedModule> {
        self.modules.iter()
    }

    /// Iterate over the modules in this set.
    #[cfg(test)]
    pub fn into_iter(self) -> std::collections::hash_set::IntoIter<LoadedModule> {
        self.modules.into_iter()
    }

    /// Get the number of modules in this set.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Determine if a module with the given source path is contained in this module set.
    pub fn contains_source_path<P>(&self, path: &P) -> bool
    where
        LoadedModule: Borrow<P>,
        P: Hash + Eq + ?Sized,
    {
        self.modules.contains(path)
    }

    /// Add a module to this set.
    ///
    /// Returns whether the module was newly inserted.
    pub fn insert_module(&mut self, module: LoadedModule) -> bool {
        self.modules.insert(module)
    }

    /// Remove a source path from this module set.
    ///
    /// Returns whether the path was present in the set.
    pub fn remove_source_path<P>(&mut self, path: &P) -> bool
    where
        LoadedModule: Borrow<P>,
        P: Hash + Eq + ?Sized,
    {
        self.modules.remove(path)
    }

    /// Get a module in this set.
    pub fn get_module<P>(&self, path: &P) -> Option<&LoadedModule>
    where
        LoadedModule: Borrow<P>,
        P: Hash + Eq + ?Sized,
    {
        self.modules.get(path)
    }

    /// Get the import name for a module.
    ///
    /// The path parameter should be relative to the GHCi session's working directory.
    pub fn get_import_name(&self, path: &NormalPath) -> Cow<'_, LoadedModule> {
        match self.get_module(path) {
            Some(module) => Cow::Borrowed(module),
            None => Cow::Owned(LoadedModule::new(path.clone())),
        }
    }
}

impl FromIterator<LoadedModule> for ModuleSet {
    fn from_iter<T: IntoIterator<Item = LoadedModule>>(iter: T) -> Self {
        Self {
            modules: iter.into_iter().collect(),
        }
    }
}

impl Extend<LoadedModule> for ModuleSet {
    fn extend<T: IntoIterator<Item = LoadedModule>>(&mut self, iter: T) {
        self.modules.extend(iter)
    }
}
