//! Socket readers/writers to support server mode.
//!
//! In the future we may want to support TCP port communication as well, but I think these will
//! generalize fairly easily.

mod connect;
mod read;
mod write;

pub use connect::SocketConnector;
pub use read::ServerCommand;
pub use read::ServerRead;
pub use write::ServerNotification;
pub use write::ServerWrite;
