use camino::Utf8PathBuf;
use miette::miette;
use winnow::combinator::repeat;
use winnow::Parser;

use super::lines::until_newline;
use super::show_paths::ShowPaths;
use super::TargetKind;

/// Parse `:show targets` output into a set of module source paths.
pub fn parse_show_targets(
    search_paths: &ShowPaths,
    input: &str,
) -> miette::Result<Vec<(Utf8PathBuf, TargetKind)>> {
    let targets: Vec<_> = repeat(0.., until_newline)
        .parse(input)
        .map_err(|err| miette!("{err}"))?;

    targets
        .into_iter()
        .map(|target| search_paths.target_to_path(target))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_show_targets() {
        let show_paths = ShowPaths {
            cwd: Utf8PathBuf::from("tests/data/simple"),
            search_paths: vec![
                Utf8PathBuf::from("tests/data/simple/test"),
                Utf8PathBuf::from("tests/data/simple/src"),
            ],
        };

        assert_eq!(
            parse_show_targets(
                &show_paths,
                indoc!(
                    "
                    src/MyLib.hs
                    MyLib.hs
                    TestMain
                    MyLib
                    MyModule
                    "
                )
            )
            .unwrap(),
            vec![
                (
                    Utf8PathBuf::from("tests/data/simple/src/MyLib.hs"),
                    TargetKind::Path
                ),
                (
                    Utf8PathBuf::from("tests/data/simple/src/MyLib.hs"),
                    TargetKind::Path
                ),
                (
                    Utf8PathBuf::from("tests/data/simple/test/TestMain.hs"),
                    TargetKind::Module
                ),
                (
                    Utf8PathBuf::from("tests/data/simple/src/MyLib.hs"),
                    TargetKind::Module
                ),
                (
                    Utf8PathBuf::from("tests/data/simple/src/MyModule.hs"),
                    TargetKind::Module
                ),
            ]
        );
    }
}
