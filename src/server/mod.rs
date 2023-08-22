//! Server mode implementation.
//!
//! These are tasks to read and write connections to notify clients of actions in `ghcid-ng` and to
//! allow clients to control and automate `ghcid-ng` themselves.

mod connect;
mod read;
mod write;

pub use connect::Server;
pub use read::ServerCommand;
pub use read::ServerRead;
pub use write::ServerNotification;
pub use write::ServerWrite;
