use std::str::FromStr;
use std::sync::OnceLock;

use miette::miette;
use regex::Regex;

/// A major version of GHC.
///
/// Variants of this enum will correspond to `ghcVersions` in `../../flake.nix`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GhcVersion {
    /// GHC 9.0
    Ghc90,
    /// GHC 9.2
    Ghc92,
    /// GHC 9.4
    Ghc94,
    /// GHC 9.6
    Ghc96,
}

fn ghc_version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([0-9]+)\.([0-9]+)\.([0-9]+)$").unwrap())
}

impl FromStr for GhcVersion {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let captures = ghc_version_re().captures(s).ok_or_else(|| {
            miette!("Failed to parse GHC version. Expected a string like \"9.6.2\", got {s:?}")
        })?;

        let (_full, [major, minor, _patch]) = captures.extract();

        match (major, minor) {
            ("9", "0") => Ok(Self::Ghc90),
            ("9", "2") => Ok(Self::Ghc92),
            ("9", "4") => Ok(Self::Ghc94),
            ("9", "6") => Ok(Self::Ghc96),
            (_, _) => Err(miette!(
                "Only GHC versions 9.0, 9.2, 9.4, and 9.6 are supported"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ghc_version() {
        assert_eq!("9.0.2".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc90);
        assert_eq!("9.2.4".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc92);
        assert_eq!("9.4.8".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc94);
        assert_eq!("9.6.1".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc96);

        "9.6.1rc1"
            .parse::<GhcVersion>()
            .expect_err("Extra information at the end");
        "9.6.1-pre"
            .parse::<GhcVersion>()
            .expect_err("Extra information at the end");
        "9.6.1.2"
            .parse::<GhcVersion>()
            .expect_err("Extra version component");
        "9.6"
            .parse::<GhcVersion>()
            .expect_err("Missing patch version component");
        "9".parse::<GhcVersion>()
            .expect_err("Missing patch and minor version components");
        "a.b.c"
            .parse::<GhcVersion>()
            .expect_err("Non-numeric components");
    }
}
