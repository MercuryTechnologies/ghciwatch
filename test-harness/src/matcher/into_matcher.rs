use crate::BaseMatcher;
use crate::Matcher;

/// A type that can be converted into a [`Matcher`] and used for searching log events.
pub trait IntoMatcher {
    /// The resulting [`Matcher`] type.
    type Matcher: Matcher;

    /// Convert the object into a `Matcher`.
    fn into_matcher(self) -> miette::Result<Self::Matcher>;
}

impl<M> IntoMatcher for M
where
    M: Matcher,
{
    type Matcher = Self;

    fn into_matcher(self) -> miette::Result<Self::Matcher> {
        Ok(self)
    }
}

impl IntoMatcher for &BaseMatcher {
    type Matcher = BaseMatcher;

    fn into_matcher(self) -> miette::Result<Self::Matcher> {
        Ok(self.clone())
    }
}

impl IntoMatcher for &str {
    type Matcher = BaseMatcher;

    fn into_matcher(self) -> miette::Result<Self::Matcher> {
        Ok(BaseMatcher::message(self))
    }
}
