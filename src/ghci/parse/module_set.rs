use std::borrow::Borrow;
use std::cmp::Eq;
use std::collections::hash_map::Keys;
use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;

use crate::normal_path::NormalPath;

use super::ShowPaths;
use super::TargetKind;

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSet {
    modules: HashMap<NormalPath, TargetKind>,
}

impl ModuleSet {
    /// Construct a `ModuleSet` from an iterator of module source paths.
    pub fn from_paths(
        paths: impl IntoIterator<Item = (impl AsRef<Path>, TargetKind)>,
        current_dir: impl AsRef<Path>,
    ) -> miette::Result<Self> {
        let current_dir = current_dir.as_ref();
        Ok(Self {
            modules: paths
                .into_iter()
                .map(|(path, kind)| {
                    NormalPath::new(path.as_ref(), current_dir).map(|path| (path, kind))
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Get the number of modules in this set.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Determine if a module with the given source path is contained in this module set.
    pub fn contains_source_path<P>(&self, path: &P) -> bool
    where
        NormalPath: Borrow<P>,
        P: Hash + Eq + ?Sized,
    {
        self.modules.contains_key(path)
    }

    /// Add a source path to this module set.
    ///
    /// Returns whether the value was newly inserted.
    pub fn insert_source_path(&mut self, path: NormalPath, kind: TargetKind) -> bool {
        self.modules.insert(path, kind).is_some()
    }

    /// Get the name used to refer to the given module path when importing it.
    ///
    /// If the module isn't imported, a path will be returned.
    ///
    /// Otherwise, the form used to import the module originally will be used. Generally this is a
    /// path if `ghciwatch` imported the module, and a module name if `ghci` imported the module on
    /// startup.
    ///
    /// See: <https://gitlab.haskell.org/ghc/ghc/-/issues/13254#note_525037>
    pub fn module_import_name(
        &self,
        show_paths: &ShowPaths,
        path: &NormalPath,
    ) -> miette::Result<(String, TargetKind)> {
        match self.modules.get(path) {
            Some(kind) => match kind {
                TargetKind::Path => Ok((path.relative().to_string(), *kind)),
                TargetKind::Module => {
                    let module = show_paths.path_to_module(path)?;
                    Ok((module, *kind))
                }
            },
            None => {
                let path = show_paths.make_relative(path)?;
                Ok((path.into_relative().into_string(), TargetKind::Path))
            }
        }
    }

    /// Iterate over the source paths in this module set.
    pub fn iter(&self) -> Keys<'_, NormalPath, TargetKind> {
        self.modules.keys()
    }
}
