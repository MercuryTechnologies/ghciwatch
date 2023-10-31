//! Parsing [`DebouncedEvent`]s into changes `ghciwatch` can respond to.

use std::collections::BTreeSet;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use notify_debouncer_full::notify::EventKind;
use notify_debouncer_full::DebouncedEvent;

/// A set of filesystem events that `ghci` will need to respond to. Due to the way that `ghci` is,
/// we need to divide these into a few different classes so that we can respond appropriately.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileEvent {
    /// Existing files that are modified, or new files that are created.
    ///
    /// `inotify` APIs aren't great at distinguishing between newly-created files and modified
    /// existing files (particularly because some editors, like `vim`, will write to a temporary
    /// file and then move that file over the original for atomicity), so this includes both sorts
    /// of changes.
    Modify(Utf8PathBuf),
    /// A file is removed.
    Remove(Utf8PathBuf),
}

impl FileEvent {
    /// Get the contained path.
    pub fn as_path(&self) -> &Utf8Path {
        match self {
            FileEvent::Modify(path) => path.as_path(),
            FileEvent::Remove(path) => path.as_path(),
        }
    }
}

/// Process a set of events into a set of [`FileEvent`]s.
pub fn file_events_from_action(events: Vec<DebouncedEvent>) -> miette::Result<BTreeSet<FileEvent>> {
    let mut ret = BTreeSet::new();

    for event in events {
        let event = event.event;
        let mut modified = false;
        let mut removed = false;
        match event.kind {
            EventKind::Remove(_) => {
                removed = true;
            }

            EventKind::Any | EventKind::Other | EventKind::Create(_) | EventKind::Modify(_) => {
                modified = true;
            }

            EventKind::Access(_) => {
                // Non-mutating event, ignore these.
            }
        }

        for path in event.paths {
            let path: Utf8PathBuf = path.try_into().into_diagnostic()?;

            if !path.exists() || removed {
                ret.insert(FileEvent::Remove(path));
            } else if modified {
                ret.insert(FileEvent::Modify(path));
            }
        }
    }

    Ok(ret)
}
