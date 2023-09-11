use camino::Utf8PathBuf;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::until_newline;

use super::GhcMessage;

/// Parse a `Loaded GHCi configuraton from /home/wiggles/.ghci` message.
pub fn loaded_configuration(input: &mut &str) -> PResult<GhcMessage> {
    let _ = "Loaded GHCi configuration from ".parse_next(input)?;
    let path = until_newline.parse_next(input)?;

    Ok(GhcMessage::LoadConfig {
        path: Utf8PathBuf::from(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_loaded_ghci_configuration_message() {
        assert_eq!(
            loaded_configuration
                .parse("Loaded GHCi configuration from /home/wiggles/.ghci\n")
                .unwrap(),
            GhcMessage::LoadConfig {
                path: "/home/wiggles/.ghci".into()
            }
        );

        // It shouldn't parse another line.
        assert!(loaded_configuration
            .parse(indoc!(
                "
                Loaded GHCi configuration from /home/wiggles/.ghci
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                "
            ))
            .is_err());
    }
}
