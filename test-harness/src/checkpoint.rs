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
    /// Convert this checkpoint into an index.
    pub fn into_index(self) -> usize {
        self.0
    }
}

impl Display for Checkpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A type that can be used to index a set of checkpoints.
pub trait CheckpointIndex: Clone {
    /// The resulting index type.
    type Index: SliceIndex<[Vec<Event>], Output = [Vec<Event>]> + Debug;

    /// Convert the value into an index.
    fn into_index(self) -> Self::Index;
}

impl CheckpointIndex for Checkpoint {
    type Index = RangeInclusive<usize>;

    fn into_index(self) -> Self::Index {
        let index = self.into_index();
        index..=index
    }
}

impl CheckpointIndex for Range<Checkpoint> {
    type Index = Range<usize>;

    fn into_index(self) -> Self::Index {
        self.start.into_index()..self.end.into_index()
    }
}

impl CheckpointIndex for RangeFrom<Checkpoint> {
    type Index = RangeFrom<usize>;

    fn into_index(self) -> Self::Index {
        self.start.into_index()..
    }
}

impl CheckpointIndex for RangeFull {
    type Index = RangeFull;

    fn into_index(self) -> Self::Index {
        self
    }
}

impl CheckpointIndex for RangeInclusive<Checkpoint> {
    type Index = RangeInclusive<usize>;

    fn into_index(self) -> Self::Index {
        self.start().into_index()..=self.end().into_index()
    }
}

impl CheckpointIndex for RangeTo<Checkpoint> {
    type Index = RangeTo<usize>;

    fn into_index(self) -> Self::Index {
        ..self.end.into_index()
    }
}

impl CheckpointIndex for RangeToInclusive<Checkpoint> {
    type Index = RangeToInclusive<usize>;

    fn into_index(self) -> Self::Index {
        ..=self.end.into_index()
    }
}
