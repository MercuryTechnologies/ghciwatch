//! Extensions and utilities for the [`ignore`] crate.

use std::path::Path;

use ignore::gitignore::Gitignore;
use ignore::gitignore::GitignoreBuilder;
use ignore::gitignore::Glob;
use ignore::Match;
use miette::Context;
use miette::IntoDiagnostic;

/// A matcher against sets of globs, `.gitignore` style.
///
/// This is mostly equivalent to [`ignore::overrides::Override`] but with an altered `matched`
/// method.
#[derive(Clone, Debug)]
pub struct GlobMatcher(Gitignore);

impl GlobMatcher {
    /// Build a glob matcher from the given iterator of glob strings.
    ///
    /// The returned matcher will match paths relative to the current directory.
    ///
    /// See the [`gitignore(5)` man page] for more details on the format and matching semantics,
    /// although note that the meaning of `!` is inverted here (globs prefixed with `!` indicate
    /// patterns of files to ignore, not patterns of files to include). In particular, note that
    /// the last matching pattern decides the match outcome.
    ///
    /// [gitignore]: https://www.man7.org/linux/man-pages/man5/gitignore.5.html
    pub fn from_globs(globs: impl IntoIterator<Item = impl AsRef<str>>) -> miette::Result<Self> {
        let mut builder = GitignoreBuilder::new(crate::current_dir()?);

        for glob in globs {
            let glob = glob.as_ref();
            builder
                .add_line(None, glob)
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to compile glob: {glob:?}"))?;
        }

        builder
            .build()
            .into_diagnostic()
            .wrap_err("Failed to compile glob matcher")
            .map(Self)
    }

    /// Returns an empty matcher that never matches any file path.
    pub fn empty() -> Self {
        Self(Gitignore::empty())
    }

    /// Returns the directory of this override set.
    ///
    /// All matches are done relative to this path.
    pub fn path(&self) -> &Path {
        self.0.path()
    }

    /// Returns true if and only if this matcher is empty.
    ///
    /// When a matcher is empty, it will never match any file path.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the total number of ignore globs.
    pub fn num_ignores(&self) -> u64 {
        self.0.num_whitelists()
    }

    /// Returns the total number of whitelisted globs.
    pub fn num_whitelists(&self) -> u64 {
        self.0.num_ignores()
    }

    /// Returns whether the given file path matched a pattern in this override matcher.
    ///
    /// If there are no overrides, then this always returns `Match::None`.
    ///
    /// Unlike [`ignore::overrides::Override::matched`], this will return `Match::None` if no globs
    /// match, even if there are whitelist overrides.
    pub fn matched<P: AsRef<Path>>(&self, path: P) -> Match<&Glob> {
        if self.0.is_empty() {
            return Match::None;
        }
        let path = path.as_ref();
        let is_dir = path.is_dir();
        self.0.matched(path, is_dir).invert()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_ext_whitelist_ignore() {
        // Globs are 'whitelist' globs by default.
        let matcher = GlobMatcher::from_globs(["puppy"]).unwrap();
        assert_eq!(matcher.num_ignores(), 0);
        assert_eq!(matcher.num_whitelists(), 1);

        // Globs starting with `!` are 'ignore' globs.
        let matcher = GlobMatcher::from_globs(["!dog"]).unwrap();
        assert_eq!(matcher.num_ignores(), 1);
        assert_eq!(matcher.num_whitelists(), 0);
    }

    #[test]
    fn test_glob_ext() {
        let matcher = GlobMatcher::from_globs([
            "config/**/*.yml",
            "config/routes.yesodroutes",
            "config/**/*.persistentmodels",
            "!config/models/*.persistentmodels",
        ])
        .unwrap();

        assert!(matcher.matched("config/dev-settings.yml").is_whitelist());
        assert!(matcher
            .matched("config/foo/dev-settings.yml")
            .is_whitelist());
        assert!(matcher.matched("/config/dev-settings.yml").is_none());
        assert!(matcher.matched("src/config/dev-settings.yml").is_none());
        assert!(matcher
            .matched("config/models/foo.persistentmodels")
            .is_ignore());
        assert!(matcher
            .matched("config/asaModels/foo.persistentmodels")
            .is_whitelist());
    }

    /// Test that the last matching pattern wins.
    #[test]
    fn test_glob_ext_ordering() {
        let matcher = GlobMatcher::from_globs([
            // Ignore all yaml files...
            "!**/*.{yml,yaml}",
            // Except for those under `config`.
            "config/**/*.{yml,yaml}",
        ])
        .unwrap();

        assert!(matcher.matched("foo.hs").is_none());
        assert!(matcher.matched("src/dev-settings.yml").is_ignore());
        assert!(matcher.matched("config/dev-settings.yml").is_whitelist());
        assert!(matcher.matched("config/dev-settings.yaml").is_whitelist());

        // This one won't match anything because the last pattern takes precedence:
        let matcher =
            GlobMatcher::from_globs(["config/**/*.{yml,yaml}", "!**/*.{yml,yaml}"]).unwrap();
        assert!(matcher.matched("foo.hs").is_none());
        assert!(matcher.matched("dev-settings.yaml").is_ignore());
        assert!(matcher.matched("config/dev-settings.yaml").is_ignore());
    }
}
