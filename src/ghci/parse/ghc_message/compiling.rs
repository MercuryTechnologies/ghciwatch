use winnow::ascii::digit1;
use winnow::ascii::space0;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::rest_of_line;
use crate::ghci::parse::module_and_files;

use super::GhcMessage;

/// Parse a `[1 of 3] Compiling Foo ( Foo.hs, Foo.o, interpreted )` message.
pub fn compiling(input: &mut &str) -> PResult<GhcMessage> {
    let _ = "[".parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let _ = digit1.parse_next(input)?;
    let _ = " of ".parse_next(input)?;
    let _ = digit1.parse_next(input)?;
    let _ = "]".parse_next(input)?;
    let _ = " Compiling ".parse_next(input)?;
    let module = module_and_files.parse_next(input)?;
    let _ = rest_of_line.parse_next(input)?;

    Ok(GhcMessage::Compiling(module))
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
            GhcMessage::Compiling(CompilingModule {
                name: "Foo".into(),
                path: "Foo.hs".into()
            })
        );

        assert_eq!(
            compiling
                .parse("[   1 of 6508] \
                    Compiling A.DoggyPrelude.Puppy \
                    ( src/A/DoggyPrelude/Puppy.hs, \
                      /Users/wiggles/doggy-web-backend6/dist-newstyle/build/aarch64-osx/ghc-9.6.2/doggy-web-backend-0/l/test-dev/noopt/build/test-dev/A/DoggyPrelude/Puppy.dyn_o \
                      ) [Doggy.Lint package changed]\n")
                .unwrap(),
            GhcMessage::Compiling(CompilingModule {
                name: "A.DoggyPrelude.Puppy".into(),
                path: "src/A/DoggyPrelude/Puppy.hs".into()
            })
        );

        assert_eq!(
            compiling
                .parse("[1 of 4] Compiling MyLib            ( src/MyLib.hs )\n")
                .unwrap(),
            GhcMessage::Compiling(CompilingModule {
                name: "MyLib".into(),
                path: "src/MyLib.hs".into()
            })
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
