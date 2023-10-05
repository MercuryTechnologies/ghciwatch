use std::collections::VecDeque;
use std::fmt::Display;
use std::ops::Range;

use line_span::LineSpanExt;
use miette::miette;
use winnow::ascii::line_ending;
use winnow::ascii::space0;
use winnow::combinator::alt;
use winnow::combinator::fold_repeat;
use winnow::combinator::opt;
use winnow::combinator::peek;
use winnow::combinator::repeat_till0;
use winnow::Located;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::GhciCommand;

use super::lines::rest_of_line;
use super::lines::until_newline;

/// A (Haskell) command for `ghciwatch` to evaluate in `ghci`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalCommand {
    /// The Haskell code or `ghci` command to evaluate.
    pub command: GhciCommand,
    /// The `command` in a user-friendly format. In particular, multiline commands aren't wrapped
    /// with `:{` and `:}`.
    display_command: String,
    /// The line number this command is from.
    line: usize,
    /// The column number this command is from.
    column: usize,
    /// The byte offsets corresponding to the span this command is from.
    byte_span: Range<usize>,
}

impl Display for EvalCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.display_command)
    }
}

/// An [`EvalCommand`] with a source code offset in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ByteSpanCommand {
    command: GhciCommand,
    display_command: String,
    span: Range<usize>,
}

/// Parse Haskell source file contents into a `Vec` of [`EvalCommand`]s to evaluate on reloads.
pub fn parse_eval_commands(contents: &str) -> miette::Result<Vec<EvalCommand>> {
    let mut byte_commands = eval_commands
        .parse(Located::new(contents))
        .map_err(|err| miette!("{err}"))?;

    // Convert the byte offsets into line and column numbers.
    //
    // This logic relies on the fact that the `byte_commands` are listed in order and
    // non-overlapping, so the offsets will be strictly ascending. It also relies on the fact that
    // there is at most one `byte_command` per line.
    //
    // Start by taking the first item in `byte_commands`, which is the first span in the file.
    let mut byte_command = match byte_commands.pop_front() {
        None => return Ok(Vec::new()),
        Some(byte_command) => byte_command,
    };

    let mut commands = Vec::with_capacity(byte_commands.len());
    for (line_span, line_number) in contents.line_spans().zip(1..) {
        // Determine if the `byte_command` starts in this line.
        let line_range = line_span.range_with_ending();
        if line_range.start <= byte_command.span.start && line_range.end > byte_command.span.start {
            // The column offset from the start of the line, in bytes.
            let column_offset = byte_command.span.start - line_range.start;
            let column_number = line_span
                .as_str_with_ending()
                // This gives us an iterator over characters and byte offsets.
                .char_indices()
                // Zip it with `1..` to get an iterator over characters, byte offsets, and
                // 1-indexed column numbers.
                .zip(1..)
                .find_map(|((char_offset, _char), column)| {
                    if column_offset <= char_offset {
                        Some(column)
                    } else {
                        None
                    }
                })
                .unwrap_or(1);
            commands.push(EvalCommand {
                command: byte_command.command,
                display_command: byte_command.display_command,
                line: line_number,
                column: column_number,
                byte_span: byte_command.span,
            });

            // Start working on the next command.
            byte_command = match byte_commands.pop_front() {
                None => return Ok(commands),
                Some(byte_command) => byte_command,
            }
        }
    }

    Ok(commands)
}

/// Parse file contents into eval commands.
fn eval_commands(input: &mut Located<&str>) -> PResult<VecDeque<ByteSpanCommand>> {
    enum Item {
        Command(ByteSpanCommand),
        Ignore,
    }

    fold_repeat(
        0..,
        alt((
            line_eval_command.map(Item::Command),
            multiline_eval_command.map(Item::Command),
            rest_of_line.map(|_| Item::Ignore),
        )),
        VecDeque::new,
        |mut commands, item| {
            match item {
                Item::Command(command) => commands.push_back(command),
                Item::Ignore => {}
            }
            commands
        },
    )
    .parse_next(input)
}

/// Parse a single-line eval command starting with `-- $> `.
///
/// Unlike `ghcid`, whitespace is allowed before the eval comment.
fn line_eval_command(input: &mut Located<&str>) -> PResult<ByteSpanCommand> {
    let _ = space0.parse_next(input)?;
    // TODO: Perhaps these eval markers should be customizable?
    let _ = "-- $> ".parse_next(input)?;
    let (command, span) = until_newline.with_span().parse_next(input)?;
    let command: GhciCommand = command.to_owned().into();

    Ok(ByteSpanCommand {
        display_command: command.clone().into(),
        command,
        span,
    })
}

/// Parse a multi-line eval command starting with `{- $>` and ending with `<$ -}`.
///
/// Unlike `ghcid`, whitespace is allowed before the eval comment.
fn multiline_eval_command(input: &mut Located<&str>) -> PResult<ByteSpanCommand> {
    let _ = space0.parse_next(input)?;
    let _ = "{- $>".parse_next(input)?;
    // Parse whitespace after the start marker and don't include it in the output command.
    let _ = space0.parse_next(input)?;
    // Ditto for a line ending after the start marker.
    let _ = opt(line_ending).parse_next(input)?;

    fn multiline_eval_end(input: &mut Located<&str>) -> PResult<()> {
        (space0, "<$ -}").void().parse_next(input)
    }
    let (command, span) =
        repeat_till0::<_, _, (), _, _, _, _>(rest_of_line, peek(multiline_eval_end))
            .recognize()
            .with_span()
            .parse_next(input)?;
    multiline_eval_end.parse_next(input)?;
    let _ = (space0, line_ending).parse_next(input)?;

    Ok(ByteSpanCommand {
        // `command` ends with a newline so we put a newline after the `:{` but not before the
        // `:}`.
        command: format!(":{{\n{command}:}}").into(),
        display_command: command.trim().to_owned(),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_line_eval_command() {
        assert_eq!(
            line_eval_command
                .parse(Located::new("-- $> foo\n"))
                .unwrap(),
            ByteSpanCommand {
                command: "foo".to_owned().into(),
                display_command: "foo".into(),
                span: 6..10,
            }
        );

        // Leading whitespace.
        assert_eq!(
            line_eval_command
                .parse(Located::new("   -- $> foo\n"))
                .unwrap(),
            ByteSpanCommand {
                command: "foo".to_owned().into(),
                display_command: "foo".into(),
                span: 9..13,
            }
        );

        // Negative cases.
        // Extra newline at end.
        assert!(line_eval_command
            .parse(Located::new("-- $> foo\n\n"))
            .is_err());
        // Multiple commands.
        assert!(line_eval_command
            .parse(Located::new(indoc!(
                "
                -- $> foo
                -- $> bar
                "
            )))
            .is_err());
    }

    #[test]
    fn test_multiline_eval_command() {
        assert_eq!(
            multiline_eval_command
                .parse(Located::new(indoc!(
                    "
                    {- $>
                    foo
                    <$ -}
                    "
                )))
                .unwrap(),
            ByteSpanCommand {
                command: indoc!(
                    "
                    :{
                    foo
                    :}"
                )
                .to_owned()
                .into(),
                display_command: "foo".into(),
                span: 6..10,
            }
        );

        // Multiple lines, command after start marker.
        assert_eq!(
            multiline_eval_command
                .parse(Located::new(indoc!(
                    "
                    {- $> puppy
                    doggy
                    kitty
                    cat
                    <$ -}
                    "
                )))
                .unwrap(),
            ByteSpanCommand {
                command: indoc!(
                    "
                    :{
                    puppy
                    doggy
                    kitty
                    cat
                    :}"
                )
                .to_owned()
                .into(),
                display_command: indoc!(
                    "puppy
                    doggy
                    kitty
                    cat"
                )
                .into(),
                span: 6..28,
            }
        );

        // Whitespace before start marker.
        assert_eq!(
            multiline_eval_command
                .parse(Located::new(indoc!(
                    "     {- $> puppy
                    <$ -}
                    "
                )))
                .unwrap(),
            ByteSpanCommand {
                command: indoc!(
                    "
                    :{
                    puppy
                    :}"
                )
                .to_owned()
                .into(),
                display_command: "puppy".into(),
                span: 11..17,
            }
        );

        assert_eq!(
            multiline_eval_command
                .parse(Located::new(indoc!(
                    "   {- $>
                        puppy
                    <$ -}
                    "
                )))
                .unwrap(),
            ByteSpanCommand {
                command: indoc!(
                    "
                    :{
                        puppy
                    :}"
                )
                .to_owned()
                .into(),
                display_command: "puppy".into(),
                span: 9..19,
            }
        );

        // Whitespace before end marker.
        assert_eq!(
            multiline_eval_command
                .parse(Located::new(indoc!(
                    "{- $> puppy
                    doggy
                            <$ -}
                    "
                )))
                .unwrap(),
            ByteSpanCommand {
                command: indoc!(
                    "
                    :{
                    puppy
                    doggy
                    :}"
                )
                .to_owned()
                .into(),
                display_command: "puppy\ndoggy".into(),
                span: 6..18,
            }
        );

        // Negative cases.
        // Markers cannot be on the same line.
        assert!(multiline_eval_command
            .parse(Located::new(indoc!(
                "
                {- $> puppy <$ -}
                "
            )))
            .is_err());

        // Extra newline at end.
        assert!(multiline_eval_command
            .parse(Located::new(indoc!(
                "
                {- $>
                puppy
                <$ -}

                "
            )))
            .is_err());

        // Text after end marker.
        assert!(multiline_eval_command
            .parse(Located::new(indoc!(
                "{- $>
                doggy
                <$ -} puppy
                "
            )))
            .is_err());

        // Two commands.
        assert!(multiline_eval_command
            .parse(Located::new(indoc!(
                "
                {- $>
                puppy
                <$ -}
                -- $> puppy
                "
            )))
            .is_err());
        assert!(multiline_eval_command
            .parse(Located::new(indoc!(
                "
                {- $>
                puppy
                <$ -}
                {- $>
                doggy
                <$ -}
                "
            )))
            .is_err());
    }

    #[test]
    fn test_parse_eval_commands() {
        assert_eq!(
            parse_eval_commands(indoc!(
                r#"
                module Foo where

                -- $> myFunc 0
                myFunc :: Int -> Int
                myFunc = id

                {- $>
                hello
                <$ -}
                -- $> goodbye

                oozy "{- $>\n\
                \this does not get parsed as an eval command\n\
                \<$ -}"

                hello =
                    {- $>
                    but this does!
                    <$ -}
                    0
                "#
            ))
            .unwrap(),
            vec![
                EvalCommand {
                    command: "myFunc 0".to_owned().into(),
                    display_command: "myFunc 0".into(),
                    line: 3,
                    column: 7,
                    byte_span: 24..33,
                },
                EvalCommand {
                    command: ":{\nhello\n:}".to_owned().into(),
                    display_command: "hello".into(),
                    line: 8,
                    column: 1,
                    byte_span: 73..79,
                },
                EvalCommand {
                    command: "goodbye".to_owned().into(),
                    display_command: "goodbye".into(),
                    line: 10,
                    column: 7,
                    byte_span: 91..99,
                },
                EvalCommand {
                    command: ":{\n    but this does!\n:}".to_owned().into(),
                    display_command: "but this does!".into(),
                    line: 18,
                    column: 1,
                    byte_span: 190..209,
                },
            ]
        )
    }
}
