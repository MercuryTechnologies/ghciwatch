//! Extensions and utilities for the [`aho_corasick`] crate.

use aho_corasick::AhoCorasick;
use aho_corasick::Anchored;
use aho_corasick::Input;
use aho_corasick::Match;
use aho_corasick::StartKind;

/// Extension trait for [`AhoCorasick`].
pub trait AhoCorasickExt {
    /// Attempt to match at the start of the input.
    fn find_at_start(&self, input: &str) -> Option<Match>;

    /// Build a matcher from the given set of patterns, with anchored matching enabled (matching at
    /// the start of the string only).
    fn from_anchored_patterns(patterns: impl IntoIterator<Item = impl AsRef<[u8]>>) -> Self;
}

impl AhoCorasickExt for AhoCorasick {
    fn find_at_start(&self, input: &str) -> Option<Match> {
        self.find(Input::new(input).anchored(Anchored::Yes))
    }

    fn from_anchored_patterns(patterns: impl IntoIterator<Item = impl AsRef<[u8]>>) -> Self {
        Self::builder()
            .start_kind(StartKind::Anchored)
            .build(patterns)
            .unwrap()
    }
}
