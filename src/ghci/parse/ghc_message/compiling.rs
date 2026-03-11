use module_and_files::CompilingModule;
use winnow::ascii::digit1;
use winnow::ascii::space0;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::rest_of_line;
use crate::ghci::parse::module_and_files;

/// Parsed result of a `[N of M] Compiling ...` line, carrying both the module info and progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilingProgress {
    pub module: CompilingModule,
    /// 1-based index of this module in the compilation batch.
    pub current: usize,
    /// Total modules in the compilation batch.
    pub total: usize,
    /// Optional recompilation reason, e.g. `[Source file changed]`.
    pub reason: Option<String>,
}

/// Parse a `[1 of 3] Compiling Foo ( Foo.hs, Foo.o, interpreted )` message.
pub fn compiling(input: &mut &str) -> PResult<CompilingProgress> {
    let _ = "[".parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let current: usize = digit1.try_map(str::parse).parse_next(input)?;
    let _ = " of ".parse_next(input)?;
    let total: usize = digit1.try_map(str::parse).parse_next(input)?;
    let _ = "]".parse_next(input)?;
    let _ = " Compiling ".parse_next(input)?;
    let module = module_and_files.parse_next(input)?;
    let remainder = rest_of_line.parse_next(input)?;
    let reason = {
        let t = remainder.trim();
        (!t.is_empty()).then(|| t.to_owned())
    };

    Ok(CompilingProgress {
        module,
        current,
        total,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use module_and_files::CompilingModule;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_compiling_message() {
        assert_eq!(
            compiling
                .parse("[1 of 3] Compiling Foo ( Foo.hs, Foo.o, interpreted )\n")
                .unwrap(),
            CompilingProgress {
                module: CompilingModule {
                    name: "Foo".into(),
                    path: "Foo.hs".into(),
                },
                current: 1,
                total: 3,
                reason: None,
            }
        );

        assert_eq!(
            compiling
                .parse("[   1 of 6508] \
                    Compiling A.DoggyPrelude.Puppy \
                    ( src/A/DoggyPrelude/Puppy.hs, \
                      /Users/wiggles/doggy-web-backend6/dist-newstyle/build/aarch64-osx/ghc-9.6.2/doggy-web-backend-0/l/test-dev/noopt/build/test-dev/A/DoggyPrelude/Puppy.dyn_o \
                      ) [Doggy.Lint package changed]\n")
                .unwrap(),
            CompilingProgress {
                module: CompilingModule {
                    name: "A.DoggyPrelude.Puppy".into(),
                    path: "src/A/DoggyPrelude/Puppy.hs".into(),
                },
                current: 1,
                total: 6508,
                reason: Some("[Doggy.Lint package changed]".into()),
            }
        );

        assert_eq!(
            compiling
                .parse("[1 of 4] Compiling MyLib            ( src/MyLib.hs )\n")
                .unwrap(),
            CompilingProgress {
                module: CompilingModule {
                    name: "MyLib".into(),
                    path: "src/MyLib.hs".into(),
                },
                current: 1,
                total: 4,
                reason: None,
            }
        );

        // Single module
        assert_eq!(
            compiling
                .parse("[1 of 1] Compiling Main ( Main.hs, interpreted )\n")
                .unwrap(),
            CompilingProgress {
                module: CompilingModule {
                    name: "Main".into(),
                    path: "Main.hs".into(),
                },
                current: 1,
                total: 1,
                reason: None,
            }
        );

        // Shouldn't parse multiple lines.
        assert!(compiling
            .parse(indoc!(
                "
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                "
            ))
            .is_err());
        assert!(compiling
            .parse(indoc!(
                "
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                "
            ))
            .is_err());
    }
}
