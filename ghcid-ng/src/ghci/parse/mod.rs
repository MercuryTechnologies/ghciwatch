//! Parsers for `ghci` output.

mod ghc_message;
mod haskell_grammar;
mod lines;
mod module_and_files;
mod module_set;

use haskell_grammar::module_name;
use lines::rest_of_line;
use lines::until_newline;
use module_and_files::module_and_files;

pub use ghc_message::parse_ghc_messages;
pub use ghc_message::CompilationResult;
pub use ghc_message::GhcMessage;
pub use ghc_message::Position;
pub use ghc_message::PositionRange;
pub use ghc_message::Severity;
pub use module_and_files::Module;
pub use module_set::ModuleSet;
