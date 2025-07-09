use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Range;
use std::ops::RangeFrom;
use std::ops::RangeFull;
use std::ops::RangeInclusive;
use std::ops::RangeTo;
use std::ops::RangeToInclusive;
use std::slice::SliceIndex;

use crate::Event;

/// A checkpoint in a `ghciwatch` run.
///
/// [`crate::GhciWatch`] provides methods for asserting that events are logged, or waiting for
/// events to be logged in the future.
///
/// To avoid searching thousands of log events for each assertion, and to provide greater
/// granularity for assertions, you can additionally assert that events are logged between
/// particular checkpoints.
///
/// Checkpoints can be constructed with [`crate::GhciWatch::first_checkpoint`],
/// [`crate::GhciWatch::current_checkpoint`], and [`crate::GhciWatch::checkpoint`].
#[derive(Debug, Clone, Copy)]
pub struct Checkpoint(pub(crate) usize);

impl Checkpoint {
    /// Get the underlying `usize` from this checkpoint.
    pub fn into_inner(self) -> usize {
        self.0
    }
}

impl Display for Checkpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A type that can be used to index a set of checkpoints.
pub trait CheckpointIndex: Clone + Debug {
    /// The resulting index type.
    type Index: SliceIndex<[Vec<Event>], Output = [Vec<Event>]> + Debug + Clone;

    /// Convert the value into an index.
    fn as_index(&self) -> Self::Index;
}

impl<C> CheckpointIndex for &C
where
    C: Clone + Debug + CheckpointIndex,
{
    type Index = <C as CheckpointIndex>::Index;

    fn as_index(&self) -> Self::Index {
        <C as CheckpointIndex>::as_index(self)
    }
}

impl CheckpointIndex for Checkpoint {
    type Index = RangeInclusive<usize>;

    fn as_index(&self) -> Self::Index {
        let index = self.into_inner();
        index..=index
    }
}

impl CheckpointIndex for Range<Checkpoint> {
    type Index = Range<usize>;

    fn as_index(&self) -> Self::Index {
        self.start.into_inner()..self.end.into_inner()
    }
}

impl CheckpointIndex for RangeFrom<Checkpoint> {
    type Index = RangeFrom<usize>;

    fn as_index(&self) -> Self::Index {
        self.start.into_inner()..
    }
}

impl CheckpointIndex for RangeFull {
    type Index = RangeFull;

    fn as_index(&self) -> Self::Index {
        *self
    }
}

impl CheckpointIndex for RangeInclusive<Checkpoint> {
    type Index = RangeInclusive<usize>;

    fn as_index(&self) -> Self::Index {
        self.start().into_inner()..=self.end().into_inner()
    }
}

impl CheckpointIndex for RangeTo<Checkpoint> {
    type Index = RangeTo<usize>;

    fn as_index(&self) -> Self::Index {
        ..self.end.into_inner()
    }
}

impl CheckpointIndex for RangeToInclusive<Checkpoint> {
    type Index = RangeToInclusive<usize>;

    fn as_index(&self) -> Self::Index {
        ..=self.end.into_inner()
    }
}
