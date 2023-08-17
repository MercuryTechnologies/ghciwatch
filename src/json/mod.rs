//! Support for reading/writing JSON. This is used to implement server-mode communication.
//!
//! In the future, this may be a fancier protocol, but for now it's just JSONL (newline-delimited
//! JSON).

mod reader;

pub use reader::JsonReader;
