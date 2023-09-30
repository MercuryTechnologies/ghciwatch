use std::fmt::Display;

use crate::Event;

mod into_matcher;
pub use into_matcher::IntoMatcher;

mod base_matcher;
pub use base_matcher::BaseMatcher;

mod or_matcher;
pub use or_matcher::OrMatcher;

mod and_matcher;
pub use and_matcher::AndMatcher;

mod fused_matcher;
pub use fused_matcher::FusedMatcher;

mod option_matcher;
pub use option_matcher::OptionMatcher;

mod never_matcher;
pub use never_matcher::NeverMatcher;

/// A type which can match log events.
pub trait Matcher: Display {
    /// Feeds an event to the matcher and determines if the matcher has finished.
    ///
    /// Note that matchers may need multiple separate log messages to complete matching.
    fn matches(&mut self, event: &Event) -> miette::Result<bool>;

    /// Construct a matcher that matches when this matcher or the `other` matcher have
    /// finished matching.
    fn or<O>(self, other: O) -> miette::Result<OrMatcher<Self, <O as IntoMatcher>::Matcher>>
    where
        O: IntoMatcher,
        Self: Sized,
    {
        Ok(OrMatcher(self, other.into_matcher()?))
    }

    /// Construct a matcher that matches when this matcher and the `other` matcher have
    /// finished matching.
    fn and<O>(
        self,
        other: O,
    ) -> miette::Result<AndMatcher<FusedMatcher<Self>, FusedMatcher<<O as IntoMatcher>::Matcher>>>
    where
        O: IntoMatcher,
        Self: Sized,
    {
        Ok(AndMatcher(self.fused(), other.into_matcher()?.fused()))
    }

    /// Construct a matcher that stops calling [`Matcher::matches`] on this matcher after it
    /// first returns `true`.
    fn fused(self) -> FusedMatcher<Self>
    where
        Self: Sized,
    {
        FusedMatcher::new(self)
    }
}

impl<M> Matcher for &mut M
where
    M: Matcher,
{
    fn matches(&mut self, event: &Event) -> miette::Result<bool> {
        (*self).matches(event)
    }
}
