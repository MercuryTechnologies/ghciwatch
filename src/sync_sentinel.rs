//! [`SyncSentinel`], a unique token that can be identified in `ghci` output.

use std::fmt::Debug;
use std::fmt::Display;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;

use tokio::sync::oneshot;

use crate::haskell_show::HaskellShow;

/// A sentinel for syncing input and output of a `ghci` session. We ask `ghci` to print this token
/// on `stdout` and then wait to see it in the `stdout` stream. This lets us know that `ghci` has
/// finished processing all of our input on `stdin` up until the point at which we asked it to
/// print this sentinel.
pub struct SyncSentinel {
    /// The sync identifier. This sets the `Display` implementation.
    id: usize,
    /// Sender, used to communicate that the sync is finished.
    sender: oneshot::Sender<()>,
}

impl SyncSentinel {
    /// Create a new `SyncSentinel` by incrementing an atomic counter.
    ///
    /// This returns a `SyncSentinel` and a [`oneshot::Receiver`] that will be notified when the
    /// sync is finshed.
    pub fn new(count: &AtomicUsize) -> (Self, oneshot::Receiver<()>) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                id: count.fetch_add(1, SeqCst),
                sender,
            },
            receiver,
        )
    }

    /// Mark this sync as finished.
    pub fn finish(self) {
        // Ignore failures if the receiver was cancelled.
        let _ = self.sender.send(());
    }
}

impl Display for SyncSentinel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "###~GHCID-NG-SYNC-{}~###", self.id)
    }
}

impl Debug for SyncSentinel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SyncSentinel").field(&self.id).finish()
    }
}

impl HaskellShow for SyncSentinel {
    fn haskell_show(&self) -> String {
        // Add a `\n` to the front to clear out anything on the line before the marker.
        let marker = self.to_string();
        let mut ret = String::with_capacity(marker.len() + 1);
        ret.push('\n');
        ret.push_str(&marker);
        ret.haskell_show()
    }
}
