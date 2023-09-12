use std::fmt::Display;

use winnow::ascii::digit1;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::combinator::preceded;
use winnow::PResult;
use winnow::Parser;

/// A position in a file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    /// 1-based line number.
    line: usize,
    /// 1-based column number.
    column: usize,
}

impl Position {
    /// Construct a new [`Position`] from a line and column number.
    #[cfg(test)]
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Is the line and column of this position zero? If so, there's no useful location information.
    pub fn is_zero(&self) -> bool {
        self.line == 0 && self.column == 0
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A range (span) of positions in a file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PositionRange {
    /// The start position.
    start: Position,
    /// The end position. If the span is zero-length, this will be the same as the start position.
    end: Position,
}

impl PositionRange {
    /// Construct a new span from the given lines and columns.
    #[cfg(test)]
    pub fn new(start_line: usize, start_column: usize, end_line: usize, end_column: usize) -> Self {
        Self {
            start: Position::new(start_line, start_column),
            end: Position::new(end_line, end_column),
        }
    }

    /// Is this a zero-length span at `0:0`?
    pub fn is_zero(&self) -> bool {
        self.start.is_zero() && self.end.is_zero()
    }
}

impl Display for PositionRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start.line == self.end.line {
            if self.start.column == self.end.column {
                write!(f, "{}", self.start)
            } else {
                write!(
                    f,
                    "{}:{}-{}",
                    self.start.line, self.start.column, self.end.column
                )
            }
        } else {
            // Different start and end lines.
            write!(
                f,
                "({},{})-({},{})",
                self.start.line, self.start.column, self.end.line, self.end.column
            )
        }
    }
}

/// A position range in a GHC diagnostic, followed by a colon.
///
/// One of these formats:
/// ```text
/// 1:2:               # Zero-length, `line:column:`
/// 1:2-4:             # Single-line, `line:startColumn-endColumn:`
/// (1,2)-(3,4):       # Multi-line, `(startLine,startColumn)-(endLine,endColumn):`
/// ```
///
/// See:
/// <https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Types/SrcLoc.hs#L348-L355>
pub fn parse_position_range(input: &mut &str) -> PResult<PositionRange> {
    fn parse_full_position_range(input: &mut &str) -> PResult<PositionRange> {
        let _ = "(".parse_next(input)?;
        let start_line = digit1.parse_to().parse_next(input)?;
        let _ = ",".parse_next(input)?;
        let start_column = digit1.parse_to().parse_next(input)?;
        let _ = ")-(".parse_next(input)?;
        let end_line = digit1.parse_to().parse_next(input)?;
        let _ = ",".parse_next(input)?;
        let end_column = digit1.parse_to().parse_next(input)?;
        let _ = ")".parse_next(input)?;
        let _ = ":".parse_next(input)?;

        Ok(PositionRange {
            start: Position {
                line: start_line,
                column: start_column,
            },
            end: Position {
                line: end_line,
                column: end_column,
            },
        })
    }

    fn parse_single_line_position_range(input: &mut &str) -> PResult<PositionRange> {
        let line = digit1.parse_to().parse_next(input)?;
        let _ = ":".parse_next(input)?;
        let start_column = digit1.parse_to().parse_next(input)?;
        // Get the end column, if any.
        let end_column = opt(preceded("-", digit1.parse_to()))
            .parse_next(input)?
            .unwrap_or(start_column);
        let _ = ":".parse_next(input)?;
        Ok(PositionRange {
            start: Position {
                line,
                column: start_column,
            },
            end: Position {
                line,
                column: end_column,
            },
        })
    }

    alt((parse_full_position_range, parse_single_line_position_range)).parse_next(input)
}

/// Parse an "unhelpful" source location like `<no location info>` followed by a colon. There's a
/// few of these.
///
/// See: https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Types/SrcLoc.hs#L251-L253
pub fn parse_unhelpful_position<'i>(input: &mut &'i str) -> PResult<&'i str> {
    alt((
        "<no location info>:",
        "<compiler-generated code>:",
        "<interactive>:",
    ))
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_position_range() {
        // Zero-length.
        assert_eq!(
            parse_position_range.parse("1:2:").unwrap(),
            PositionRange::new(1, 2, 1, 2)
        );
        assert_eq!(
            parse_position_range.parse("4258:12859:").unwrap(),
            PositionRange::new(4258, 12859, 4258, 12859)
        );

        // Single-line.
        assert_eq!(
            parse_position_range.parse("1:2-3:").unwrap(),
            PositionRange::new(1, 2, 1, 3)
        );
        assert_eq!(
            parse_position_range.parse("621:284-312:").unwrap(),
            PositionRange::new(621, 284, 621, 312)
        );

        // Multi-line.
        assert_eq!(
            parse_position_range.parse("(1,2)-(3,4):").unwrap(),
            PositionRange::new(1, 2, 3, 4)
        );
        assert_eq!(
            parse_position_range.parse("(04,30)-(19,98):").unwrap(),
            PositionRange::new(4, 30, 19, 98)
        );
        assert_eq!(
            parse_position_range.parse("(571,643)-(5466,123):").unwrap(),
            PositionRange::new(571, 643, 5466, 123)
        );

        // Negative cases.
        // Whitespace:
        assert!(parse_position_range.parse(" 1:2:").is_err());
        assert!(parse_position_range.parse("1:2: ").is_err());
        assert!(parse_position_range.parse("1 :2:").is_err());
        assert!(parse_position_range.parse("1: 2:").is_err());
        assert!(parse_position_range.parse("1:2 :").is_err());
        assert!(parse_position_range.parse("1:2 -3:").is_err());
        assert!(parse_position_range.parse("(1, 2)-(3, 4):").is_err());
        assert!(parse_position_range.parse("(1,2) - (3,4):").is_err());
        assert!(parse_position_range.parse(" (1,2)-(3,4):").is_err());

        // Missing parens:
        assert!(parse_position_range.parse("1,2-3,4:").is_err());
        // Extra parens:
        assert!(parse_position_range.parse("(1:2):").is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(Position::new(1, 1).to_string(), "1:1");
        assert_eq!(Position::new(19, 98).to_string(), "19:98");

        assert_eq!(PositionRange::new(1, 1, 1, 1).to_string(), "1:1");
        assert_eq!(PositionRange::new(2, 3, 2, 3).to_string(), "2:3");
        assert_eq!(PositionRange::new(2, 3, 2, 5).to_string(), "2:3-5");
        assert_eq!(PositionRange::new(12, 3, 12, 28).to_string(), "12:3-28");
        assert_eq!(PositionRange::new(2, 3, 4, 5).to_string(), "(2,3)-(4,5)");
    }
}
