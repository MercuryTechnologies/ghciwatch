use winnow::combinator::dispatch;
use winnow::combinator::fail;
use winnow::combinator::success;
use winnow::combinator::terminated;
use winnow::token::take_until1;
use winnow::PResult;
use winnow::Parser;

/// The severity of a compiler message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Warning-level; non-fatal.
    Warning,
    /// Error-level; fatal.
    Error,
}

/// Parse a severity followed by a `:`, either `Warning` or `Error`.
pub fn parse_severity_colon(input: &mut &str) -> PResult<Severity> {
    terminated(
        dispatch! {take_until1(":");
            "warning"|"Warning" => success(Severity::Warning),
            "error"|"Error" => success(Severity::Error),
            _ => fail,
        },
        ":",
    )
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_severity() {
        assert_eq!(
            parse_severity_colon.parse("Warning:").unwrap(),
            Severity::Warning
        );
        assert_eq!(
            parse_severity_colon.parse("warning:").unwrap(),
            Severity::Warning
        );
        assert_eq!(
            parse_severity_colon.parse("Error:").unwrap(),
            Severity::Error
        );
        assert_eq!(
            parse_severity_colon.parse("error:").unwrap(),
            Severity::Error
        );

        // Negative cases.
        assert!(parse_severity_colon.parse(" Error:").is_err());
        assert!(parse_severity_colon.parse("Error :").is_err());
        assert!(parse_severity_colon.parse("Error: ").is_err());
        assert!(parse_severity_colon.parse(" Warning:").is_err());
        assert!(parse_severity_colon.parse("Warning :").is_err());
        assert!(parse_severity_colon.parse("Warning: ").is_err());
        assert!(parse_severity_colon.parse("W arning:").is_err());
    }
}
