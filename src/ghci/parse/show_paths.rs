use std::collections::HashSet;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use eyre::eyre;
use itertools::Itertools;
use winnow::ascii::newline;
use winnow::ascii::space0;
use winnow::ascii::space1;
use winnow::combinator::opt;
use winnow::combinator::preceded;
use winnow::combinator::repeat;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::loaded_module::LoadedModule;
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
    /// Convert a target (from `:show targets` output) to a module source path.
    pub fn target_to_path(&self, target: &str) -> eyre::Result<LoadedModule> {
        let target_path = Utf8Path::new(target);
        if is_haskell_source_file(target_path) {
            // The target is already a path.
            let path = self.cwd.join(target_path);
            tracing::trace!(%path, %target, "Target is path");
            return Ok(LoadedModule::new(NormalPath::new(path, &self.cwd)?));
        } else {
            // Else, split by `.` to get path components.
            let mut path = target.split('.').collect::<Utf8PathBuf>();

            // Try each extension, starting with `.hs`.
            for haskell_source_extension in HASKELL_SOURCE_EXTENSIONS {
                path.set_extension(haskell_source_extension);

                for search_path in self.absolute_search_paths() {
                    let path = search_path.join(&path);
                    if path.exists() {
                        tracing::trace!(%path, %target, "Found path for target");
                        return Ok(LoadedModule::with_name(
                            NormalPath::new(path, &self.cwd)?,
                            target.to_owned(),
                        ));
                    }
                }
            }
        }
        Err(eyre!("Couldn't find source path for {target}"))
    }

    fn absolute_search_paths(&self) -> impl Iterator<Item = Utf8PathBuf> + '_ {
        self.search_paths.iter().map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                self.cwd.join(path)
            }
        })
    }

    /// Convert a Haskell source path to a module name.
    pub fn path_to_module(&self, path: &Utf8Path) -> eyre::Result<String> {
        let path = path.with_extension("");
        let path_str = path.as_str();

        for search_path in self.absolute_search_paths() {
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

        Err(eyre!("Couldn't convert {path} to module name"))
    }
}

/// Parse `:show paths` output into a set of module search paths.
pub fn parse_show_paths(input: &str) -> eyre::Result<ShowPaths> {
    let mut show_paths = show_paths.parse(input).map_err(|err| eyre!("{err}"))?;

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
    let first_unit_show_paths = show_single_unit_paths.parse_next(input)?;
    let other_unit_show_paths: Vec<ShowPaths> =
        repeat(0.., show_single_unit_paths).parse_next(input)?;

    // assume that working directory for first unit listed is good enough
    let cwd = first_unit_show_paths.cwd;
    let search_paths = first_unit_show_paths
        .search_paths
        .into_iter()
        .chain(
            other_unit_show_paths
                .into_iter()
                .flat_map(|show_paths| show_paths.search_paths),
        )
        .collect();

    Ok(ShowPaths { cwd, search_paths })
}

fn show_single_unit_paths(input: &mut &str) -> PResult<ShowPaths> {
    let _ = opt(("Unit ID: ", until_newline.map(Utf8PathBuf::from))).parse_next(input)?;
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
                    Unit ID: my-simple-package-0.1.0.0-inplace-test-dev
                    current working directory:
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite
                    module import search paths:
                      test
                      src
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/test-dev
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/test-dev/autogen
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/global-autogen
                    Unit ID: my-simple-package-0.1.0.0-inplace-test-lib
                    current working directory:
                      /Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite
                    module import search paths:
                      test
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/test-lib
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/test-lib/autogen
                      dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/global-autogen
                    "
                ))
                .unwrap(),
            ShowPaths {
                cwd: Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite"),
                search_paths: vec![
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/test"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/src"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/test-dev"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/test-dev/autogen"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-dev/build/global-autogen"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/test"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/test-lib"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/test-lib/autogen"),
                      Utf8PathBuf::from("/Users/wiggles/ghciwatch/ghciwatch/tests/data/with-test-suite/dist-newstyle/build/x86_64-linux/ghc-9.14.1/my-simple-package-0.1.0.0/l/test-lib/build/global-autogen"),
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
            search_paths: vec![Utf8PathBuf::from("src")],
        };

        assert_eq!(
            paths
                .path_to_module(Utf8Path::new("/Users/wiggles/ghciwatch/src/Foo/Bar/Baz.hs"))
                .unwrap(),
            "Foo.Bar.Baz"
        );
    }
}
