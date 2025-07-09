use std::fmt::Display;
use std::str::FromStr;
use std::sync::OnceLock;

use miette::miette;
use regex::Regex;

/// A GHC version, including the patch level.
pub struct FullGhcVersion {
    /// The major version.
    pub major: GhcVersion,
    /// The full version string.
    pub full: String,
}

impl FullGhcVersion {
    /// Get the GHC version for the current test.
    pub fn current() -> miette::Result<Self> {
        let full = crate::internal::get_ghc_version()?;
        let major = full.parse()?;
        Ok(Self { full, major })
    }
}

impl Display for FullGhcVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.full)
    }
}

/// A major version of GHC.
///
/// Variants of this enum will correspond to `ghcVersions` in `../../flake.nix`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GhcVersion {
    /// GHC 9.4
    Ghc94,
    /// GHC 9.6
    Ghc96,
    /// GHC 9.8
    Ghc98,
    /// GHC 9.10
    Ghc910,
    /// GHC 9.12
    Ghc912,
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
            ("9", "4") => Ok(Self::Ghc94),
            ("9", "6") => Ok(Self::Ghc96),
            ("9", "8") => Ok(Self::Ghc98),
            ("9", "10") => Ok(Self::Ghc910),
            ("9", "12") => Ok(Self::Ghc912),
            (_, _) => Err(miette!(
                "Only the following GHC versions are supported:\n\
                - 9.4\n\
                - 9.6\n\
                - 9.8\n\
                - 9.10\n\
                - 9.12"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ghc_version() {
        assert_eq!("9.4.8".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc94);
        assert_eq!("9.6.1".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc96);
        assert_eq!("9.10.1".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc910);
        assert_eq!("9.12.1".parse::<GhcVersion>().unwrap(), GhcVersion::Ghc910);

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
