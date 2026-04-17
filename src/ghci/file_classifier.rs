//! Classifies file events into reload actions based on glob patterns.

use std::collections::BTreeSet;
use std::path::Path;

use camino::Utf8PathBuf;

use crate::event_filter::FileEvent;
use crate::haskell_source_file::is_haskell_source_file;
use crate::ignore::GlobMatcher;
use crate::normal_path::NormalPath;

use super::module_set::ModuleSet;
use super::GhciReloadKind;

/// Classifies file events into reload actions based on glob patterns
/// and the current state of loaded modules.
///
/// This struct can be used both as a component of [`super::Ghci`] and standalone
/// (e.g., during GHCi initialization, before the full session is available).
#[derive(Debug, Clone)]
pub struct FileClassifier {
    /// Restart the `ghci` session when paths matching these globs are changed.
    restart_globs: GlobMatcher,
    /// Reload the `ghci` session when paths matching these globs are changed.
    reload_globs: GlobMatcher,
    /// The working directory used to make paths relative.
    cwd: Utf8PathBuf,
}

impl FileClassifier {
    /// Construct a new `FileClassifier` from glob matchers, using the process's
    /// current working directory.
    ///
    /// This is suitable for use before GHCi has initialized.
    pub fn new(restart_globs: GlobMatcher, reload_globs: GlobMatcher) -> miette::Result<Self> {
        Ok(Self {
            restart_globs,
            reload_globs,
            cwd: crate::current_dir_utf8()?,
        })
    }

    /// Update the working directory (typically after `:show paths` is parsed).
    pub fn set_cwd(&mut self, cwd: Utf8PathBuf) {
        self.cwd = cwd;
    }

    /// Make a path relative to the working directory.
    pub fn relative_path(&self, path: impl AsRef<Path>) -> miette::Result<NormalPath> {
        NormalPath::new(path, &self.cwd)
    }

    /// Classify a set of file events into reload actions.
    ///
    /// `targets` is the set of currently loaded modules. Pass `&ModuleSet::default()` when
    /// GHCi has not yet initialized (all Haskell modifications will be classified as "add").
    pub(crate) fn classify(
        &self,
        events: BTreeSet<FileEvent>,
        targets: &ModuleSet,
    ) -> miette::Result<ReloadActions> {
        // Once we know which paths were modified and which paths were removed, we can combine
        // that with information about this `ghci` session to determine which modules need to be
        // reloaded, which modules need to be added, and which modules were removed. In the case
        // of removed modules, the entire `ghci` session must be restarted.
        let mut needs_restart = Vec::new();
        let mut needs_reload = Vec::new();
        let mut needs_add = Vec::new();
        let mut needs_remove = Vec::new();
        for event in events {
            let path = event.as_path();
            let path = self.relative_path(path)?;

            let restart_match = self.restart_globs.matched(&path);
            let reload_match = self.reload_globs.matched(&path);
            let path_is_haskell_source_file = is_haskell_source_file(&path);
            tracing::trace!(
                ?event,
                ?restart_match,
                ?reload_match,
                is_haskell_source_file = path_is_haskell_source_file,
                "Checking path"
            );

            // Don't restart if we've explicitly ignored this path in a glob.
            if !restart_match.is_ignore()
                // Restart on `.cabal` and `.ghci` files.
                && (path
                    .extension()
                    .map(|ext| ext == "cabal")
                    .unwrap_or(false)
                || path
                    .file_name()
                    .map(|name| name == ".ghci")
                    .unwrap_or(false)
                // Restart on explicit restart globs.
                || restart_match.is_whitelist())
            {
                // Restart for this path.
                tracing::debug!(%path, "Needs restart");
                needs_restart.push(path);
            } else if reload_match.is_ignore() {
                // Ignoring this path, continue.
            } else if matches!(event, FileEvent::Remove(_))
                && path_is_haskell_source_file
                && targets.contains_source_path(&path)
            {
                tracing::debug!(%path, "Needs remove");
                needs_remove.push(path);
            } else if matches!(event, FileEvent::Modify(_))
                && path_is_haskell_source_file
                // Unless a user explicitly asks for it (e.g. `--reload-glob '**/.*.hs`), ignore
                // dotfiles when reloading.
                && !path
                    .file_name()
                    .is_some_and(|name| name.starts_with('.'))
            {
                // Otherwise, reload when Haskell files are modified.
                if targets.contains_source_path(&path) {
                    // We can `:reload` paths in the target set.
                    tracing::debug!(%path, "Needs reload");
                    needs_reload.push(path);
                } else {
                    // Otherwise we need to `:add` the new paths.
                    tracing::debug!(%path, "Needs add");
                    needs_add.push(path);
                }
            } else if reload_match.is_whitelist() {
                // Extra extensions are always reloaded, never added.
                tracing::debug!(%path, "Needs reload");
                needs_reload.push(path);
            }
        }

        Ok(ReloadActions {
            needs_restart,
            needs_reload,
            needs_add,
            needs_remove,
        })
    }
}

/// Actions needed to perform a reload.
///
/// See [`super::Ghci::reload`].
#[derive(Debug)]
pub(crate) struct ReloadActions {
    /// Paths to modules which need a full `ghci` restart.
    pub needs_restart: Vec<NormalPath>,
    /// Paths to modules which need a `:reload`.
    pub needs_reload: Vec<NormalPath>,
    /// Paths to modules which need an `:add`.
    pub needs_add: Vec<NormalPath>,
    /// Paths to modules which need an `:unadd`.
    pub needs_remove: Vec<NormalPath>,
}

impl ReloadActions {
    /// Do any modules need to be added, removed, or reloaded?
    pub fn needs_modify(&self) -> bool {
        !self.needs_add.is_empty() || !self.needs_reload.is_empty() || !self.needs_remove.is_empty()
    }

    /// Is a session restart needed?
    pub fn needs_restart(&self) -> bool {
        !self.needs_restart.is_empty()
    }

    /// Get the kind of reload we'll perform.
    pub fn kind(&self) -> GhciReloadKind {
        if self.needs_restart() {
            GhciReloadKind::Restart
        } else if self.needs_modify() {
            GhciReloadKind::Reload
        } else {
            GhciReloadKind::None
        }
    }
}
