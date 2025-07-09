use miette::miette;
use winnow::combinator::repeat;
use winnow::Parser;

use crate::ghci::ModuleSet;

use super::lines::until_newline;
use super::show_paths::ShowPaths;

/// Parse `:show targets` output into a set of module source paths.
pub fn parse_show_targets(search_paths: &ShowPaths, input: &str) -> miette::Result<ModuleSet> {
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
    use std::collections::HashSet;

    use crate::ghci::loaded_module::LoadedModule;
    use crate::normal_path::NormalPath;

    use super::*;
    use camino::Utf8PathBuf;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_show_targets() {
        let show_paths = ShowPaths {
            cwd: NormalPath::from_cwd("tests/data/simple")
                .unwrap()
                .absolute()
                .to_owned(),
            search_paths: vec![Utf8PathBuf::from("test"), Utf8PathBuf::from("src")],
        };

        let normal_path = |p: &str| NormalPath::new(p, &show_paths.cwd).unwrap();

        assert_eq!(
            parse_show_targets(
                &show_paths,
                indoc!(
                    "
                    src/MyLib.hs
                    TestMain
                    MyLib
                    MyModule
                    "
                )
            )
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>(),
            vec![
                LoadedModule::new(normal_path("src/MyLib.hs")),
                LoadedModule::with_name(normal_path("test/TestMain.hs"), "TestMain".to_owned()),
                LoadedModule::with_name(normal_path("src/MyLib.hs"), "MyLib".to_owned()),
                LoadedModule::with_name(normal_path("src/MyModule.hs"), "MyModule".to_owned()),
            ]
            .into_iter()
            .collect()
        );
    }
}
