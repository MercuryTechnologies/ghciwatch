/// Entries in `:show targets` can be one of two types: module paths or module names (with `.` in
/// place of path separators). Due to a `ghci` bug, the module can only be referred to as whichever
/// form it was originally added as (see below), so we use this to track how we refer to modules.
///
/// See: <https://gitlab.haskell.org/ghc/ghc/-/issues/13254#note_525037>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    /// A target named by its source path.
    Path,
    /// A target named by its module name.
    Module,
}
