use std::collections::HashSet;
use std::str::FromStr;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::miette;
use miette::IntoDiagnostic;

use crate::event_filter::HASKELL_SOURCE_EXTENSIONS;
use crate::lines::Lines;

/// Information about a Haskell module loaded in a `ghci` session. These are parsed from `:show
/// modules` output via the [`FromStr`] trait.
///
/// For reference, a line of `:show modules` output looks like this:
/// ```plain
/// A.MercuryPrelude ( src/A/MercuryPrelude.hs, /Users/wiggles/mwb4/dist-newstyle/build/aarch64-osx/ghc-9.6.1/mwb-0/l/test-dev/noopt/build/test-dev/A/MercuryPrelude.dyn_o )
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The module's fully-qualified name.
    pub name: String,
    /// The path to the module's source file, typically a `.hs` file.
    pub source: Utf8PathBuf,
    /// Paths of the module's output files, including `.dyn_o`, `.o`, and so on.
    pub outputs: Vec<Utf8PathBuf>,
}

impl FromStr for Module {
    type Err = miette::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut tokens = s.split_ascii_whitespace();

        let name = match tokens.next() {
            Some(name) => name.to_owned(),
            None => {
                return Err(miette!(
                    "`:show modules` output line doesn't include a module name: {s:?}"
                ));
            }
        };

        match tokens.next() {
            Some(paren) => {
                if paren != "(" {
                    return Err(miette!("`:show modules` output line is malformed; expected \"(\", got {paren:?}: {s:?}"));
                }
            }
            None => {
                return Err(miette!(
                    "`:show modules` output line doesn't include a list of files"
                ));
            }
        }

        let mut source: Option<&str> = None;
        let mut outputs = Vec::new();

        while let Some(token) = tokens.next() {
            if token == ")" {
                // We're done.
                match tokens.next() {
                    Some(_) => {
                        return Err(miette!("`:show modules` output line includes token(s) after the closing parenthesis: {s:?}"));
                    }
                    None => {
                        break;
                    }
                }
            }

            // Remove a trailing comma, if one exists.
            // If you have commas in your file names, G-d help you!
            let path = token.strip_suffix(',').unwrap_or(token);

            if HASKELL_SOURCE_EXTENSIONS
                .iter()
                .any(|extension| path.ends_with(&format!(".{extension}")))
            {
                source = Some(path);
            } else {
                outputs.push(path.into());
            }
        }

        let source = source
            .ok_or_else(|| {
                miette!("Didn't find a source file in `:show modules` output line: {s:?}")
            })?
            .into();

        Ok(Self {
            name,
            source,
            outputs,
        })
    }
}

/// A collection of source paths, retaining information about loaded modules in a `ghci`
/// session.
#[derive(Debug, Clone, Default)]
pub struct ModuleSet {
    map: HashSet<Utf8PathBuf>,
}

impl ModuleSet {
    /// Parse a `ModuleSet` from a set of lines read from `ghci` stdout.
    pub fn from_lines(lines: &Lines) -> miette::Result<Self> {
        Ok(Self {
            map: lines
                .iter()
                .map(|line| {
                    line.parse::<Module>().and_then(|module| {
                        match module.source.canonicalize_utf8() {
                            Ok(absolute_path) => Ok(absolute_path),
                            Err(err) => Err(err).into_diagnostic(),
                        }
                    })
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Get the number of modules in this set.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Determine if a module with the given source path is contained in this module set.
    pub fn contains_source_path(&self, path: &Utf8Path) -> bool {
        self.map.contains(path)
    }

    /// Add a source path to this module set.
    pub fn insert_source_path(&mut self, path: Utf8PathBuf) {
        self.map.insert(path);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_parse_module() {
        assert_eq!(
            "A.MercuryPrelude ( src/A/MercuryPrelude.hs, /Users/wiggles/mwb4/dist-newstyle/build/aarch64-osx/ghc-9.6.1/mwb-0/l/test-dev/noopt/build/test-dev/A/MercuryPrelude.dyn_o )".parse::<Module>().unwrap(),
            Module {
                name: "A.MercuryPrelude".into(),
                source: "src/A/MercuryPrelude.hs".into(),
                outputs: vec!["/Users/wiggles/mwb4/dist-newstyle/build/aarch64-osx/ghc-9.6.1/mwb-0/l/test-dev/noopt/build/test-dev/A/MercuryPrelude.dyn_o".into()],
            }
        );
    }
}
