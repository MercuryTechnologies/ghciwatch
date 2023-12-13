use std::collections::HashSet;
use std::path::Path;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::miette;
use winnow::ascii::newline;
use winnow::ascii::space0;
use winnow::ascii::space1;
use winnow::combinator::opt;
use winnow::combinator::preceded;
use winnow::combinator::repeat;
use winnow::PResult;
use winnow::Parser;

use crate::haskell_source_file::is_haskell_source_file;
use crate::haskell_source_file::HASKELL_SOURCE_EXTENSIONS;
use crate::normal_path::NormalPath;

use super::lines::until_newline;

/// Parsed `:show paths` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowPaths {
    /// The current working directory.
    pub cwd: Utf8PathBuf,
    /// Module import search paths.
    pub search_paths: Vec<Utf8PathBuf>,
}

impl ShowPaths {
    /// Make a path relative to the working directory of this session.
    pub fn make_relative(&self, path: impl AsRef<Path>) -> miette::Result<NormalPath> {
        NormalPath::new(path, &self.cwd)
    }

    /// Convert a target (from `:show targets` output) to a module source path.
    pub fn target_to_path(&self, target: &str) -> miette::Result<Utf8PathBuf> {
        let target_path = Utf8Path::new(target);
        if is_haskell_source_file(target_path) {
            // The target is already a path.
            if let Some(path) = self.target_path_to_path(target_path) {
                tracing::trace!(%path, %target, "Target is path");
                return Ok(path);
            }
        } else {
            // Else, split by `.` to get path components.
            let mut path = target.split('.').collect::<Utf8PathBuf>();

            // Try each extension, starting with `.hs`.
            for haskell_source_extension in HASKELL_SOURCE_EXTENSIONS {
                path.set_extension(haskell_source_extension);

                if let Some(path) = self.target_path_to_path(&path) {
                    tracing::trace!(%path, %target, "Found path for target");
                    return Ok(path);
                }
            }
        }
        Err(miette!("Couldn't find source path for {target}"))
    }

    /// Convert a target path like `src/MyLib.hs` to a module source path starting with one of the
    /// `search_paths`.
    fn target_path_to_path(&self, target: &Utf8Path) -> Option<Utf8PathBuf> {
        for search_path in self.paths() {
            let path = search_path.join(target);
            if path.exists() {
                // Found it!
                return Some(path);
            }
        }

        None
    }

    fn paths(&self) -> impl Iterator<Item = &Utf8PathBuf> {
        self.search_paths.iter().chain(std::iter::once(&self.cwd))
    }

    /// Convert a Haskell source path to a module name.
    pub fn path_to_module(&self, path: &Utf8Path) -> miette::Result<String> {
        let path = path.with_extension("");
        let path_str = path.as_str();

        for search_path in self.paths() {
            if let Some(suffix) = path_str.strip_prefix(search_path.as_str()) {
                let module_name = Utf8Path::new(suffix)
                    .components()
                    .filter_map(|component| match component {
                        camino::Utf8Component::Normal(part) => Some(part),
                        _ => None,
                    })
                    .join(".");
                return Ok(module_name);
            }
        }

        Err(miette!("Couldn't convert {path} to module name"))
    }
}

/// Parse `:show paths` output into a set of module search paths.
pub fn parse_show_paths(input: &str) -> miette::Result<ShowPaths> {
    let mut show_paths = show_paths.parse(input).map_err(|err| miette!("{err}"))?;

    // Deduplicate the search paths.
    show_paths.search_paths = show_paths
        .search_paths
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    Ok(show_paths)
}

fn show_paths(input: &mut &str) -> PResult<ShowPaths> {
    let _ = "current working directory:".parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let _ = newline.parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let cwd = until_newline.map(Utf8PathBuf::from).parse_next(input)?;
    let _ = "module import search paths:".parse_next(input)?;

    // Special case for no search paths.
    // See: https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/ghc/GHCi/UI.hs#L3452
    let no_import_paths = opt(" none\n").parse_next(input)?;
    if no_import_paths.is_some() {
        return Ok(ShowPaths {
            cwd,
            search_paths: Vec::new(),
        });
    }

    let _ = newline.parse_next(input)?;
    let search_paths = repeat(
        0..,
        preceded(
            space1,
            until_newline.map(|path| {
                let path = Utf8PathBuf::from(path);
                // If the path is relative, like `test`, join it to the working directory.
                if path.is_relative() {
                    cwd.join(path)
                } else {
                    path
                }
            }),
        ),
    )
    .parse_next(input)?;

    Ok(ShowPaths { cwd, search_paths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_show_paths() {
        assert_eq!(
            show_paths
                .parse(indoc!(
                    "
                    current working directory:
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple
                    module import search paths:
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/test-dev
                      test
                      src
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/test-dev/autogen
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/global-autogen
                    "
                ))
                .unwrap(),
            ShowPaths {
                cwd: Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple"),
                search_paths: vec![
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/test-dev"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/test"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/src"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/test-dev/autogen"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple/dist-newstyle/build/aarch64-osx/ghc-9.0.2/my-simple-package-0.1.0.0/l/test-dev/build/global-autogen"),
                ],
            }
        );

        assert_eq!(
            show_paths
                .parse(indoc!(
                    "
                    current working directory:
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple
                    module import search paths: none
                    "
                ))
                .unwrap(),
            ShowPaths {
                cwd: Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/simple"),
                search_paths: vec![],
            }
        );

        // Negative cases.
        // Path after "none"
        assert!(show_paths
            .parse(indoc!(
                "
                current working directory:
                  /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple
                module import search paths: none
                  /Foo/bar
                "
            ))
            .is_err());

        // No leading whitespace.
        assert!(show_paths
            .parse(indoc!(
                "
                current working directory:
                  /Users/wiggles/ghciwatch/ghciwatch/tests/data/simple
                module import search paths:
                  /Foo/bar
                /Foo/bar
                "
            ))
            .is_err());
    }

    #[test]
    fn test_path_to_module() {
        let paths = ShowPaths {
            cwd: Utf8PathBuf::from("/Users/wiggles/ghciwatch/"),
            search_paths: vec![],
        };

        assert_eq!(
            paths
                .path_to_module(Utf8Path::new("/Users/wiggles/ghciwatch/Foo/Bar/Baz.hs"))
                .unwrap(),
            "Foo.Bar.Baz"
        );
    }
}
