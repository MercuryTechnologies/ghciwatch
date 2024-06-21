use std::str::FromStr;

use camino::Utf8PathBuf;
use miette::miette;
use winnow::combinator::alt;
use winnow::combinator::rest;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::haskell_grammar::module_name;
use crate::ghci::parse::haskell_source_file;
use crate::ghci::parse::lines::line_ending_or_eof;

use super::single_quoted::single_quoted;

/// A module or file was not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotFound {
    /// A file for import was not found.
    ///
    /// ```text
    /// File src/Foo.hs not found
    /// ```
    File(Utf8PathBuf),

    /// A module for import was not found.
    ///
    /// ```text
    /// Module Foo not found
    /// ```
    Module(String),

    /// A target was not found as a source file or recognized as a module name.
    ///
    /// ```text
    /// target ‘src/Puppy’ is not a module name or a source file
    /// ```
    Unrecognized(String),
}

impl FromStr for NotFound {
    type Err = miette::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        not_found.parse(s).map_err(|err| miette!("{err}"))
    }
}

/// Parse a `File src/Foo.hs not found` message.
fn file_not_found(input: &mut &str) -> PResult<Utf8PathBuf> {
    let _ = "File ".parse_next(input)?;
    let (path, _) = haskell_source_file(" not found").parse_next(input)?;
    let _ = line_ending_or_eof.parse_next(input)?;
    Ok(path)
}

/// Parse a `Module Foo not found` message.
fn module_not_found(input: &mut &str) -> PResult<String> {
    let _ = "Module ".parse_next(input)?;
    let module = module_name(input)?;
    let _ = " not found".parse_next(input)?;
    let _ = line_ending_or_eof.parse_next(input)?;
    Ok(module.to_owned())
}

/// Parse a `target 'Foo' is not a module name or a source file` message.
fn unrecognized(input: &mut &str) -> PResult<String> {
    let _ = "target ".parse_next(input)?;
    let (name, _) = single_quoted(
        rest,
        (" is not a module name or a source file", line_ending_or_eof),
    )
    .parse_next(input)?;
    Ok(name.to_owned())
}

pub fn not_found(input: &mut &str) -> PResult<NotFound> {
    alt((
        file_not_found.map(NotFound::File),
        module_not_found.map(NotFound::Module),
        unrecognized.map(NotFound::Unrecognized),
    ))
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_module_not_found() {
        assert_eq!(
            "Module Puppy not found".parse::<NotFound>().unwrap(),
            NotFound::Module("Puppy".into())
        );

        assert_eq!(
            "Module Puppy not found\n".parse::<NotFound>().unwrap(),
            NotFound::Module("Puppy".into())
        );

        assert_eq!(
            "Module Puppy.Doggy' not found\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Module("Puppy.Doggy'".into())
        );

        // Negative cases.
        assert!("Module Puppy Doggy not found\n"
            .parse::<NotFound>()
            .is_err());
        assert!("Module Puppy\\ Doggy not found\n"
            .parse::<NotFound>()
            .is_err());
        assert!("Module Puppy*.Doggy not found\n"
            .parse::<NotFound>()
            .is_err());
        assert!("Module Puppy.Doggy not\n".parse::<NotFound>().is_err());
        assert!("Module Puppy\n.Doggy not found\n"
            .parse::<NotFound>()
            .is_err());
    }

    #[test]
    fn test_parse_file_not_found() {
        assert_eq!(
            "File src/Puppy.hs not found".parse::<NotFound>().unwrap(),
            NotFound::File("src/Puppy.hs".into())
        );

        assert_eq!(
            "File src/Puppy.hs not found\n".parse::<NotFound>().unwrap(),
            NotFound::File("src/Puppy.hs".into())
        );

        assert_eq!(
            "File src/ Puppy.hs not found\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::File("src/ Puppy.hs".into())
        );

        assert_eq!(
            "File src/\nPuppy.hs not found\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::File("src/\nPuppy.hs".into())
        );

        assert_eq!(
            "File src/Puppy.hs.lhs not found\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::File("src/Puppy.hs.lhs".into())
        );

        assert_eq!(
            "File src/Puppy.hs not foun.lhs not found\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::File("src/Puppy.hs not foun.lhs".into())
        );

        // Negative cases.

        // No extension.
        assert!("File src/Puppy not found\n".parse::<NotFound>().is_err());

        // Non-Haskell extension.
        assert!("File src/Puppy.x not found\n".parse::<NotFound>().is_err());
        assert!("File src/Puppy.hs.bak not found\n"
            .parse::<NotFound>()
            .is_err());

        // Extra punctuation.
        assert!("File src/Puppy.hs not found!\n"
            .parse::<NotFound>()
            .is_err());

        // Case sensitivity.
        assert!("file src/Puppy.hs not found!\n"
            .parse::<NotFound>()
            .is_err());
    }

    #[test]
    fn test_parse_unrecognized_not_found() {
        // The input and quoting here is maddeningly open-ended so there's a ton of these cases.

        assert_eq!(
            "target ‘src/Puppy’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Puppy".into()),
        );

        // Newline at end.
        assert_eq!(
            "target ‘src/Puppy’ is not a module name or a source file\n"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Puppy".into()),
        );

        // Empty string.
        assert_eq!(
            "target ‘’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("".into()),
        );

        // Whitespace.
        assert_eq!(
            "target ‘src/ Puppy’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/ Puppy".into()),
        );
        assert_eq!(
            "target ‘ src/Puppy’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized(" src/Puppy".into()),
        );
        assert_eq!(
            "target ‘src/Puppy ’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Puppy ".into()),
        );

        // Internal quotes!
        assert_eq!(
            "target ‘src/Pupp'y’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Pupp'y".into()),
        );
        assert_eq!(
            "target ‘src/Pupp'''y’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Pupp'''y".into()),
        );
        assert_eq!(
            "target ‘'src/Puppy'’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("'src/Puppy'".into()),
        );
        assert_eq!(
            "target ‘‘src/Puppy’’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("‘src/Puppy’".into()),
        );

        // Newlines oh my!
        assert_eq!(
            "target ‘src\n/Puppy\n’ is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src\n/Puppy\n".into()),
        );

        // ASCII quotes.
        assert_eq!(
            "target `src/Puppy' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Puppy".into()),
        );
        assert_eq!(
            "target ``src/Puppy' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("`src/Puppy".into()),
        );
        assert_eq!(
            "target `src/Pupp'y`' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Pupp'y`".into()),
        );

        assert_eq!(
            "target 'src/Puppy' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("'src/Puppy'".into()),
        );
        assert_eq!(
            "target 'src/Pupp'y' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("'src/Pupp'y'".into()),
        );
        assert_eq!(
            "target src/Puppy' is not a module name or a source file"
                .parse::<NotFound>()
                .unwrap(),
            NotFound::Unrecognized("src/Puppy'".into()),
        );
    }
}
