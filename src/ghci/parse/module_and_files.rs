use std::str::FromStr;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use miette::miette;
use winnow::ascii::space1;
use winnow::combinator::repeat;
use winnow::error::AddContext;
use winnow::error::ContextError;
use winnow::error::ErrMode;
use winnow::error::StrContext;
use winnow::error::StrContextValue;
use winnow::token::take_till;
use winnow::token::take_until;
use winnow::PResult;
use winnow::Parser;

use crate::haskell_source_file::is_haskell_source_file;

use super::module_name;

/// Information about a Haskell module loaded in a `ghci` session. These are parsed from `:show
/// modules` output or from compiler output directly.
///
/// For reference, a line of `:show modules` output looks like this:
/// ```text
/// A.MercuryPrelude ( src/A/MercuryPrelude.hs, /Users/wiggles/mwb4/dist-newstyle/build/aarch64-osx/ghc-9.6.1/mwb-0/l/test-dev/noopt/build/test-dev/A/MercuryPrelude.dyn_o )
/// ```
///
/// And a line of compiler output looks like this:
/// ```text
/// Compiling A.Puppy.Doggy ( src/A/Puppy/Doggy.hs, dist-newstyle/A/Puppy/Doggy.o, interpreted ) [Doggy.Lint package changed]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The module's fully-qualified name.
    pub name: String,
    /// The path to the module's source file, typically a `.hs` file.
    pub path: Utf8PathBuf,
}

impl FromStr for Module {
    type Err = miette::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        module_and_files.parse(s).map_err(|err| miette!("{err}"))
    }
}

/// Parse a Haskell module name and list of files, like this:
///
/// ```text
/// My.Cool.Module ( src/My/Cool/Module.hs, dist-newstyle/My/Cool/Module.o, interpreted )
/// ```
pub fn module_and_files(input: &mut &str) -> PResult<Module> {
    let module_name = module_name(input)?;
    let _ = space1.parse_next(input)?;

    // Parse this bit: `( Foo.hs, Foo.o, interpreted )`
    let _ = "( ".parse_next(input)?;
    let mut paths: Vec<_> = repeat(0.., (take_till(1.., (',', '\n')), ", ")).parse_next(input)?;
    let final_path = (take_until(1.., " )"), " )").parse_next(input)?;
    paths.push(final_path);

    let mut module_path = None;
    for (path, _comma) in paths {
        if is_haskell_source_file(Utf8Path::new(path)) {
            module_path = Some(Utf8PathBuf::from(path));
            break;
        }
    }

    match module_path {
        None => Err(ErrMode::Cut(ContextError::new().add_context(
            input,
            StrContext::Expected(StrContextValue::Description("a Haskell source file path")),
        ))),
        Some(path) => Ok(Module {
            name: module_name.to_owned(),
            path,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_module() {
        assert_eq!(
            "A.MercuryPrelude ( src/A/MercuryPrelude.hs, /Users/wiggles/mwb4/dist-newstyle/build/aarch64-osx/ghc-9.6.1/mwb-0/l/test-dev/noopt/build/test-dev/A/MercuryPrelude.dyn_o )".parse::<Module>().unwrap(),
            Module {
                name: "A.MercuryPrelude".into(),
                path: "src/A/MercuryPrelude.hs".into(),
            }
        );
    }

    #[test]
    fn test_parse_module_and_files() {
        assert_eq!(
            module_and_files
                .parse("Foo ( Foo.hs, Foo.o, interpreted )")
                .unwrap(),
            Module {
                name: "Foo".into(),
                path: "Foo.hs".into()
            }
        );

        assert_eq!(
            module_and_files
                .parse("A.DoggyPrelude.Puppy \
                    ( src/A/DoggyPrelude/Puppy.hs, \
                      /Users/wiggles/doggy-web-backend6/dist-newstyle/build/aarch64-osx/ghc-9.6.2/doggy-web-backend-0/l/test-dev/noopt/build/test-dev/A/DoggyPrelude/Puppy.dyn_o \
                      )")
                .unwrap(),
            Module{
                name: "A.DoggyPrelude.Puppy".into(),
                path: "src/A/DoggyPrelude/Puppy.hs".into()
            }
        );

        assert_eq!(
            module_and_files
                .parse("MyLib            ( src/MyLib.hs )")
                .unwrap(),
            Module {
                name: "MyLib".into(),
                path: "src/MyLib.hs".into()
            }
        );

        // Shouldn't parse multiple lines.
        assert!(module_and_files
            .parse(indoc!(
                "
                MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                "
            ))
            .is_err());
        assert!(module_and_files
            .parse(indoc!(
                "
                MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                "
            ))
            .is_err());
    }
}
