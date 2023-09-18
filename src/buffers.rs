//! Constants for buffer sizes.
//!
//! This is kind of awkward, but marginally better than writing `1024` everywhere?
//! Time will tell if we need more granular tuning than this.

/// The default capacity (in bytes) of buffers storing a line of text.
///
/// This should be large enough to accomodate most lines of output without resizing the buffer.
/// We also don't need to allocate that many buffers at once, so it's fine for this to be
/// substantially larger than the defaults. (IIRC the default sizes of `Vec`s and `String`s allow
/// something like a dozen entries or so.)
pub const LINE_BUFFER_CAPACITY: usize = 1024;

/// The default capacity (in entries) of buffers storing a collection of items, usually lines.
pub const VEC_BUFFER_CAPACITY: usize = 1024;
