//! Haskell source file and path tools.

use camino::Utf8Path;

/// File extensions for Haskell source code.
///
/// See: <https://downloads.haskell.org/ghc/latest/docs/users_guide/using.html#meaningful-file-suffixes>
///
/// See: <https://gitlab.haskell.org/ghc/ghc/-/blob/077cb2e11fa81076e8c9c5f8dd3bdfa99c8aaf8d/compiler/GHC/Driver/Phases.hs#L236-L252>
pub const HASKELL_SOURCE_EXTENSIONS: [&str; 8] = [
    // NOTE: This should start with `hs` so that iterators try the most common extension first.
    "hs",       // Haskell
    "lhs",      // Literate Haskell
    "hs-boot", // See: https://downloads.haskell.org/ghc/latest/docs/users_guide/separate_compilation.html#how-to-compile-mutually-recursive-modules
    "lhs-boot", // Literate `hs-boot`.
    "hsig", // Backpack module signatures: https://ghc.gitlab.haskell.org/ghc/doc/users_guide/separate_compilation.html#module-signatures
    "lhsig", // Literate backpack module signatures.
    "hspp", // "A file created by the preprocessor".
    "hscpp", // Haskell C-preprocessor files.
];

/// Determine if a given path represents a Haskell source file.
pub fn is_haskell_source_file(path: impl AsRef<Utf8Path>) -> bool {
    let path = path.as_ref();
    // Haskell source files end in a known extension.
    path.extension()
        .map(|ext| HASKELL_SOURCE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
        // Haskell source files do not start with `.` (Emacs swap files in particular start with
        // `.#`).
        && path
            .file_name()
            .map_or(false, |name| !name.starts_with('.'))
}
