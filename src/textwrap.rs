//! [`textwrap`] helpers.

use std::borrow::Cow;

use tap::Tap;
use textwrap::Options;
use textwrap::WordSeparator;
use textwrap::WordSplitter;

/// Get [`textwrap`] options with our settings.
pub fn options<'a>() -> Options<'a> {
    let opts = Options::with_termwidth()
        .break_words(false)
        .word_separator(WordSeparator::AsciiSpace)
        .word_splitter(WordSplitter::NoHyphenation);

    // In tests, the terminal is always 80 characters wide.
    if cfg!(test) {
        opts.with_width(80)
    } else {
        opts
    }
}

/// Extension trait adding methods to [`textwrap::Options`]
pub trait TextWrapOptionsExt {
    /// Subtract from the current `width`.
    fn subtract_width(self, decrease: usize) -> Self;

    /// Set the `width` to wrap the text to.
    fn with_width(self, width: usize) -> Self;

    /// Wrap the given text into lines.
    fn wrap<'s>(&self, text: &'s str) -> Vec<Cow<'s, str>>;

    /// Wrap the given text into lines and return a `String`.
    ///
    /// Like [`wrap`] but with the lines pre-joined.
    fn fill(&self, text: &str) -> String;
}

impl<'a> TextWrapOptionsExt for Options<'a> {
    fn subtract_width(self, decrease: usize) -> Self {
        self.clone().tap_mut(|o| o.width = self.width - decrease)
    }

    fn with_width(self, width: usize) -> Self {
        self.clone().tap_mut(|o| o.width = width)
    }

    fn wrap<'s>(&self, text: &'s str) -> Vec<Cow<'s, str>> {
        textwrap::wrap(text, self)
    }

    fn fill(&self, text: &str) -> String {
        textwrap::fill(text, self)
    }
}
