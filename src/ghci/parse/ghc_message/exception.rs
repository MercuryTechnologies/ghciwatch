use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::until_newline;

use super::GhcMessage;

/// Parse an "Exception" message like this:
///
/// ```plain
/// *** Exception: /Users/.../dist-newstyle/ghc82733_tmp_1/ghc_tmp_34657.h: withFile: does not exist (No such file or directory)
/// ```
pub fn exception(input: &mut &str) -> PResult<GhcMessage> {
    let _ = "*** Exception: ".parse_next(input)?;

    let message = until_newline.parse_next(input)?;

    Ok(GhcMessage::Exception(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_exception() {
        assert_eq!(
            exception.parse("*** Exception: Uh oh!\n").unwrap(),
            GhcMessage::Exception("Uh oh!".into())
        );

        assert_eq!(
            exception.parse("*** Exception: /Users/.../dist-newstyle/ghc82733_tmp_1/ghc_tmp_34657.h: withFile: does not exist (No such file or directory)\n").unwrap(),
            GhcMessage::Exception("/Users/.../dist-newstyle/ghc82733_tmp_1/ghc_tmp_34657.h: withFile: does not exist (No such file or directory)".into())
        );

        // Doesn't parse subsequent lines (even if they're relevant, unfortunately).
        assert_eq!(
            exception
                .parse(indoc!(
                    "
                    *** Exception: puppy doggy
                    CallStack (from HasCallStack):
                      error, called at <interactive>:3:1 in interactive:Ghci1
                    "
                ))
                .unwrap(),
            GhcMessage::Exception("puppy doggy".into())
        );
    }
}
