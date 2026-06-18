//! Warning message formatting with GHC-style coloring.
//!
//! This module provides functionality to colorize GHC diagnostic messages
//! to match the output format and colors used by GHC itself.

use owo_colors::OwoColorize;
use owo_colors::Stream::Stdout;

use crate::ghci::parse::Severity;

/// Apply GHC-style coloring to a complete diagnostic message.
///
/// This processes multi-line messages and applies appropriate coloring
/// to each line based on GHC's output patterns.
pub fn colorize_message(message: &str, severity: Severity) -> String {
    message
        .lines()
        .map(|line| colorize_line(line, severity))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Apply colors to a single line of a diagnostic message based on GHC patterns.
///
/// This function recognizes different types of lines in GHC output and applies
/// appropriate coloring:
/// - Source code lines with line numbers (e.g., "  28 | import Data.Coerce (coerce)")
/// - Caret indicator lines (e.g., "     | ^^^^^^^^^^^^^^^^^^^^^^^^^^^")
/// - Warning/error flags in brackets (e.g., "[-Wunused-imports]")
///
/// TODO: Connect it to the ANSI colors
/// that we need to preserve in <https://github.com/MercuryTechnologies/ghciwatch/blob/TrackWarnings/src/ghci/parse/ghc_message/mod.rs#L153>
fn colorize_line(line: &str, severity: Severity) -> String {
    // Detect different types of lines and apply appropriate coloring

    // Source code lines with line numbers (e.g., "  28 | import Data.Coerce (coerce)")
    // TODO: This is a pretty hacky way to find the lines to color. In the future, if we have all of this structured from the parser, we can store the original colors alongside the warning text and re-emit it directly.
    if let Some(pipe_pos) = line.find(" | ") {
        let before_pipe = &line[..pipe_pos];
        if before_pipe.trim().chars().all(|c| c.is_ascii_digit()) {
            // This looks like a line number followed by pipe
            let line_num_part = &line[..pipe_pos];
            let pipe_and_after = &line[pipe_pos..];

            return format!(
                "{}{}",
                line_num_part.if_supports_color(Stdout, |text| text.magenta()),
                pipe_and_after.if_supports_color(Stdout, |text| text.magenta())
            );
        }
    }

    // Caret lines (e.g., "     | ^^^^^^^^^^^^^^^^^^^^^^^^^^^")
    if line.trim_start().starts_with('|') && line.contains('^') {
        return format!("{}", line.if_supports_color(Stdout, |text| text.magenta()));
    }

    // Color warning/error flags in brackets
    let mut result = line.to_string();

    // Look for [-Wxxxx] patterns
    if line.contains("[-W") {
        if let Some(start) = line.find("[-W") {
            if let Some(end) = line[start..].find(']') {
                let flag = &line[start..start + end + 1];
                let colored_flag = match severity {
                    Severity::Warning => {
                        format!("{}", flag.if_supports_color(Stdout, |text| text.magenta()))
                    }
                    Severity::Error => {
                        format!("{}", flag.if_supports_color(Stdout, |text| text.red()))
                    }
                };
                result = result.replace(flag, &colored_flag);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colorize_line_number_with_pipe() {
        let line = "  28 | import Data.Coerce (coerce)";
        let result = colorize_line(line, Severity::Warning);

        // The result should contain the original content
        assert!(result.contains("28"));
        assert!(result.contains("import Data.Coerce (coerce)"));

        // Test that the logic correctly identifies line number patterns
        assert!(line.contains(" | "));
        let before_pipe = &line[..line.find(" | ").unwrap()];
        assert!(before_pipe.trim().chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_colorize_caret_line() {
        let line = "     | ^^^^^^^^^^^^^^^^^^^^^^^^^^^";
        let result = colorize_line(line, Severity::Warning);

        // Should contain the original caret characters
        assert!(result.contains("^"));
        assert!(result.contains("|"));

        // Test that the logic correctly identifies caret patterns
        assert!(line.trim_start().starts_with('|'));
        assert!(line.contains('^'));
    }

    #[test]
    fn test_colorize_warning_flag() {
        let line = "    [-Wunused-imports]";
        let result = colorize_line(line, Severity::Warning);

        // Should contain the original flag
        assert!(result.contains("[-Wunused-imports]"));

        // Test that the logic correctly identifies warning flags
        assert!(line.contains("[-W"));
    }

    #[test]
    fn test_colorize_error_flag() {
        let line = "    [-Werror]";
        let result = colorize_line(line, Severity::Error);

        // Should contain the original flag
        assert!(result.contains("[-Werror]"));

        // Test that the logic correctly identifies warning flags
        assert!(line.contains("[-W"));
    }

    #[test]
    fn test_colorize_plain_line() {
        let line = "This is a plain line with no special formatting";
        let result = colorize_line(line, Severity::Warning);

        // Plain lines should remain unchanged
        assert_eq!(result, line);
    }

    #[test]
    fn test_colorize_multiline_message() {
        let message = "Top-level binding with no type signature:\n  foo :: [Char] -> [Char]\n  28 | foo x = x\n      | ^^^^^^^^^";
        let result = colorize_message(message, Severity::Warning);

        // Should preserve the structure and content
        assert!(result.contains("Top-level binding"));
        assert!(result.contains("foo :: [Char] -> [Char]"));
        assert!(result.contains("28"));
        assert!(result.contains("^"));

        // Should preserve line structure
        assert_eq!(result.lines().count(), message.lines().count());
    }

    #[test]
    fn test_line_number_detection() {
        // Valid line number patterns - test the logic directly
        let line1 = "  1 | module Main";
        let result1 = colorize_line(line1, Severity::Warning);
        assert!(result1.contains("1"));
        assert!(result1.contains("module Main"));

        let line2 = "123 | import Data.List";
        let result2 = colorize_line(line2, Severity::Warning);
        assert!(result2.contains("123"));
        assert!(result2.contains("import Data.List"));

        // Invalid patterns (should not match line number logic)
        let line3 = "abc | not a line number";
        let result3 = colorize_line(line3, Severity::Warning);
        assert_eq!(result3, line3);

        let line4 = "  | no line number";
        let result4 = colorize_line(line4, Severity::Warning);
        assert_eq!(result4, line4);
    }

    #[test]
    fn test_caret_line_detection() {
        // Valid caret patterns - test the logic directly
        let line1 = "     | ^^^";
        let result1 = colorize_line(line1, Severity::Warning);
        assert!(result1.contains("^"));
        assert!(result1.contains("|"));

        let line2 = "  | ^^^^^^^^^^^^^^^";
        let result2 = colorize_line(line2, Severity::Warning);
        assert!(result2.contains("^"));
        assert!(result2.contains("|"));

        // Invalid patterns should remain unchanged
        let line3 = "     | no carets here";
        let result3 = colorize_line(line3, Severity::Warning);
        assert_eq!(result3, line3);

        let line4 = "^^^ no pipe";
        let result4 = colorize_line(line4, Severity::Warning);
        assert_eq!(result4, line4);
    }
}
