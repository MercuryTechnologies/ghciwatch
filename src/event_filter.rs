//! Parsing [`watchexec::event::Event`]s into changes `ghcid-ng` can respond to.

use std::collections::HashMap;

use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use watchexec::action::Action;
use watchexec::event::filekind::CreateKind;
use watchexec::event::filekind::FileEventKind;
use watchexec::event::filekind::ModifyKind;
use watchexec::event::filekind::RemoveKind;
use watchexec::event::Event;
use watchexec::event::Tag;

/*

# Notes on reacting to file events

- All file events are tagged `Source(Filesystem)`.
- `file_type` is `None` for temporary files used as buffers for atomic writes, like `src/4913` or `src/App.hs~`
  ...or in general, files that are removed?

  Looks like it's populated by the metadata for the path:
  https://github.com/watchexec/watchexec/blob/402d1ba4f900b5e744e33d56b824e92833d60613/crates/lib/src/fs.rs#L314

Events when I write an existing source file in vim:
• FileEventKind(Modify(Name(Any))),           "/Users/wiggles/mwb4/src/App.hs",  file_type: Some(File)
• FileEventKind(Create(File)),                "/Users/wiggles/mwb4/src/App.hs",  file_type: Some(File)
• FileEventKind(Modify(Data(Content))),       "/Users/wiggles/mwb4/src/App.hs",  file_type: Some(File)
• FileEventKind(Modify(Metadata(Ownership))), "/Users/wiggles/mwb4/src/App.hs",  file_type: Some(File)
• FileEventKind(Modify(Name(Any))),           "/Users/wiggles/mwb4/src/App.hs~", file_type: None
• FileEventKind(Remove(File)),                "/Users/wiggles/mwb4/src/App.hs~", file_type: None
• FileEventKind(Create(File)),                "/Users/wiggles/mwb4/src/4913",    file_type: None
• FileEventKind(Remove(File)),                "/Users/wiggles/mwb4/src/4913",    file_type: None
• FileEventKind(Modify(Metadata(Ownership))), "/Users/wiggles/mwb4/src/4913",    file_type: None
^ This is technically a rename due to How It Works. We can tell it's not a "real rename" because
  there's no removed `.hs` files.
^ I think the only real way to tell if it's a rename or not is to check if the paths are in the
  loaded module set or not?

Events when I write an existing source file in VSCode:
• FileEventKind(Modify(Metadata(Any))) "/Users/wiggles/mwb4/src/App.hs" file_type: Some(File)
• FileEventKind(Modify(Data(Content))) "/Users/wiggles/mwb4/src/App.hs" file_type: Some(File)
^ A `Modify(Data(Content))` and `Modify(Metadata(_))` and nothing else for that path.

Events when I create a new file:
• FileEventKind(Create(Folder)),        "/Users/wiggles/mwb4/src/Foo",             file_type: Some(Dir)
• FileEventKind(Create(File)),          "/Users/wiggles/mwb4/src/Foo/MyModule.hs", file_type: Some(File)
• FileEventKind(Modify(Data(Content))), "/Users/wiggles/mwb4/src/Foo/MyModule.hs", file_type: Some(File)
^ There's a `Create(File)` and `Modify(Data(_))`and nothing else for that path.

Events when I `rm -rf` a directory:
• FileEventKind(Remove(File)),   "/Users/wiggles/mwb4/src/Foo/MyModule.hs", file_type: None
• FileEventKind(Remove(Folder)), "/Users/wiggles/mwb4/src/Foo",             file_type: None
^ Note that these both have `file_type: None`.
^ There's a `Remove(File)` event and nothing else for that path.

Events when I `mv` a module to a new location:
• FileEventKind(Create(File)),          "/Users/wiggles/mwb4/src/Foo/MyModule.hs",     file_type: None
• FileEventKind(Modify(Data(Content))), "/Users/wiggles/mwb4/src/Foo/MyModule.hs",     file_type: None
• FileEventKind(Modify(Name(Any))),     "/Users/wiggles/mwb4/src/Foo/MyModule.hs",     file_type: None
• FileEventKind(Modify(Name(Any))),     "/Users/wiggles/mwb4/src/Foo/MyCoolModule.hs", file_type: Some(File)
^ There's a `Modify(Name(_))` event for the new and old paths. The new path has a `file_type: Some(File)`.
^ No good way to tell _what_ file it was renamed from.
^ Why is the old (removed) file marked as `Create(File)`???

 */

/// File extensions for Haskell source code.
pub const HASKELL_SOURCE_EXTENSIONS: [&str; 9] = [
    "hs",      // Haskell
    "lhs",     // Literate Haskell
    "hsboot",  // Haskell boot file.
    "hs-boot", // See: https://downloads.haskell.org/ghc/latest/docs/users_guide/separate_compilation.html#how-to-compile-mutually-recursive-modules
    "hsc", // `hsc2hs` C bindings: https://downloads.haskell.org/ghc/latest/docs/users_guide/utils.html?highlight=interfaces#writing-haskell-interfaces-to-c-code-hsc2hs
    "x",   // `alex` (lexer generator): https://hackage.haskell.org/package/alex
    "y",   // `happy` (parser generator): https://hackage.haskell.org/package/happy
    "c2hs", // `c2hs` C bindings: https://hackage.haskell.org/package/c2hs
    "gc",  // `greencard` C bindings: https://hackage.haskell.org/package/greencard
];

/// A filesystem event that `ghci` will need to respond to. Due to the way that `ghci` is, we need
/// to divide these into a few different classes so that we can respond appropriately.
#[derive(Debug)]
pub enum FileEvent {
    /// An existing file is modified, or a new file is created.
    ///
    /// `inotify` APIs aren't great at distinguishing between newly-created files and modified
    /// existing files (particularly because some editors, like `vim`, will write to a temporary
    /// file and then move that file over the original for atomicity), so this includes both sorts
    /// of changes.
    Modify(Utf8PathBuf),
    /// A file is removed.
    Remove(Utf8PathBuf),
}

/// Process the events contained in an [`Action`] into a list of [`FileEvent`]s.
pub fn file_events_from_action(action: &Action) -> miette::Result<Vec<FileEvent>> {
    // First, build up a map from paths to events tagged with that path.
    // This will give us easy access to the events for a given path in this batch.
    let mut events_by_path = HashMap::<Utf8PathBuf, Vec<&Event>>::new();
    for event in action.events.iter() {
        for tag in event.tags.iter() {
            if let Tag::Path { path, .. } = tag {
                let path = path.to_owned().try_into().into_diagnostic()?;
                let entry = events_by_path.entry(path).or_insert_with(Vec::new);
                entry.push(event);
                // No need to look at the rest of the tags.
                break;
            }
        }
    }

    let mut ret = Vec::new();

    for (path, events) in events_by_path.iter() {
        if path
            .extension()
            .map(|ext| !HASKELL_SOURCE_EXTENSIONS.contains(&ext))
            .unwrap_or(true)
        {
            // If the path doesn't have a Haskell source extension, we don't need to process it.
            // In the future, we'll want something more sophisticated here -- we'll need to reload
            // for non-Haskell files or even run commands when non-Haskell files change -- but this
            // is fine for a first pass.
            continue;
        }

        let mut exists = false;
        let mut created = false;
        let mut modified = false;
        let mut removed = false;
        let mut renamed = false;
        for event in events {
            for tag in event.tags.iter() {
                match tag {
                    Tag::Path { path: _, file_type } => {
                        exists = file_type.is_some();
                    }
                    Tag::FileEventKind(FileEventKind::Modify(ModifyKind::Name(_))) => {
                        renamed = true;
                    }
                    Tag::FileEventKind(FileEventKind::Modify(ModifyKind::Data(_))) => {
                        modified = true;
                    }
                    Tag::FileEventKind(FileEventKind::Create(CreateKind::File)) => {
                        created = true;
                    }
                    Tag::FileEventKind(FileEventKind::Remove(RemoveKind::File)) => {
                        removed = true;
                    }
                    _ => {}
                }
            }
        }

        // Write existing file from Vim:    exists, renamed, created, modified
        // Write existing file from VSCode: exists,                   modified
        // Create a new file:               exists,          created, modified
        // `mv`:                            exists, renamed
        // `rm -rf`:                        !exists,                            removed
        //
        // We can't distinguish between modifying an existing file and creating a new one.
        //
        // We could probably just use `modified` and ignore the `created` and `renamed` values
        // here.

        if !exists && removed {
            ret.push(FileEvent::Remove(path.clone()));
        } else if modified || created || renamed {
            ret.push(FileEvent::Modify(path.clone()));
        }
    }

    Ok(ret)
}
