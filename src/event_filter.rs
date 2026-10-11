//! Parsing [`DebouncedEvent`]s into changes `ghciwatch` can respond to.

use std::collections::BTreeSet;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use notify_debouncer_full::notify::EventKind;
use notify_debouncer_full::DebouncedEvent;

/// A set of filesystem events that `ghci` will need to respond to. Due to the way that `ghci` is,
/// we need to divide these into a few different classes so that we can respond appropriately.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
pub fn file_events_from_action(events: Vec<DebouncedEvent>) -> eyre::Result<BTreeSet<FileEvent>> {
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
            let path: Utf8PathBuf = path.try_into()?;

            // Trust the filesystem over the event kind. `git checkout`, `git stash`, and editors
            // that save by deleting and recreating a file produce a `Remove` event for a path
            // that exists again by the time we see the batch. Treating that as a removal would
            // `:unadd` a module that still exists, dropping it (and anything only it imports)
            // from the session.
            if !path.exists() {
                ret.insert(FileEvent::Remove(path));
            } else if modified || removed {
                ret.insert(FileEvent::Modify(path));
            }
        }
    }

    Ok(ret)
}

/// Merging event batches can produce both a [`FileEvent::Remove`] and a [`FileEvent::Modify`]
/// for one path: it was missing when one batch was read and back when a later batch was read.
/// Keep only the event that matches the filesystem now, as [`file_events_from_action`] does
/// within one batch.
pub fn reconcile_file_events(events: &mut BTreeSet<FileEvent>) {
    let conflicting = events
        .iter()
        .filter_map(|event| match event {
            FileEvent::Remove(path) if events.contains(&FileEvent::Modify(path.clone())) => {
                Some(path.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for path in conflicting {
        if path.exists() {
            events.remove(&FileEvent::Remove(path));
        } else {
            events.remove(&FileEvent::Modify(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Instant;

    use notify_debouncer_full::notify::event::CreateKind;
    use notify_debouncer_full::notify::event::RemoveKind;
    use notify_debouncer_full::notify::Event;
    use pretty_assertions::assert_eq;

    fn debounced(kind: EventKind, path: &Utf8Path) -> DebouncedEvent {
        DebouncedEvent::new(
            Event::new(kind).add_path(path.as_std_path().to_path_buf()),
            Instant::now(),
        )
    }

    fn temp_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();
        (dir, path)
    }

    /// `git checkout` and `git stash` delete and recreate files; that's not a removal.
    #[test]
    fn remove_then_recreate_is_a_modify() {
        let (_dir, dir_path) = temp_dir();
        let path = dir_path.join("Foo.hs");
        std::fs::write(&path, "module Foo where\n").unwrap();
        let events = file_events_from_action(vec![
            debounced(EventKind::Remove(RemoveKind::File), &path),
            debounced(EventKind::Create(CreateKind::File), &path),
        ])
        .unwrap();
        assert_eq!(events, BTreeSet::from([FileEvent::Modify(path)]));
    }

    #[test]
    fn remove_of_missing_file_is_a_remove() {
        let (_dir, dir_path) = temp_dir();
        let path = dir_path.join("Gone.hs");
        let events =
            file_events_from_action(vec![debounced(EventKind::Remove(RemoveKind::File), &path)])
                .unwrap();
        assert_eq!(events, BTreeSet::from([FileEvent::Remove(path)]));
    }

    /// A path removed in one batch and recreated in the next is reloaded, not removed.
    #[test]
    fn reconcile_merged_remove_and_modify() {
        let (_dir, dir_path) = temp_dir();
        let exists = dir_path.join("Exists.hs");
        let gone = dir_path.join("Gone.hs");
        let other = dir_path.join("Other.hs");
        std::fs::write(&exists, "module Exists where\n").unwrap();

        let mut events = BTreeSet::from([
            FileEvent::Remove(exists.clone()),
            FileEvent::Modify(exists.clone()),
            FileEvent::Remove(gone.clone()),
            FileEvent::Modify(gone.clone()),
            FileEvent::Remove(other.clone()),
        ]);
        reconcile_file_events(&mut events);
        assert_eq!(
            events,
            BTreeSet::from([
                FileEvent::Modify(exists),
                FileEvent::Remove(gone),
                FileEvent::Remove(other),
            ])
        );
    }
}
