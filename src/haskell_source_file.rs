//! Haskell source file and path tools.

use camino::Utf8Path;

/// File extensions for Haskell source code.
pub const HASKELL_SOURCE_EXTENSIONS: [&str; 9] = [
    // NOTE: This should start with `hs` so that iterators try the most common extension first.
    "hs",      // Haskell
    "lhs",     // Literate Haskell
    "hs-boot", // See: https://downloads.haskell.org/ghc/latest/docs/users_guide/separate_compilation.html#how-to-compile-mutually-recursive-modules
    "hsig", // Backpack module signatures: https://ghc.gitlab.haskell.org/ghc/doc/users_guide/separate_compilation.html#module-signatures
    "hsc", // `hsc2hs` C bindings: https://downloads.haskell.org/ghc/latest/docs/users_guide/utils.html?highlight=interfaces#writing-haskell-interfaces-to-c-code-hsc2hs
    "x",   // `alex` (lexer generator): https://hackage.haskell.org/package/alex
    "y",   // `happy` (parser generator): https://hackage.haskell.org/package/happy
    "c2hs", // `c2hs` C bindings: https://hackage.haskell.org/package/c2hs
    "gc",  // `greencard` C bindings: https://hackage.haskell.org/package/greencard
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
