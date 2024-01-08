//! Parsers for `ghci` output and Haskell code.

mod eval;
mod ghc_message;
mod haskell_grammar;
mod lines;
mod module_and_files;
mod module_set;
mod show_paths;
mod show_targets;

use haskell_grammar::module_name;
use lines::rest_of_line;
use module_and_files::module_and_files;

pub use eval::parse_eval_commands;
pub use eval::EvalCommand;
pub use ghc_message::parse_ghc_messages;
pub use ghc_message::CompilationResult;
pub use ghc_message::CompilationSummary;
pub use ghc_message::GhcDiagnostic;
pub use ghc_message::GhcMessage;
pub use ghc_message::Severity;
pub use module_and_files::Module;
pub use module_set::ModuleSet;
pub use show_paths::parse_show_paths;
pub use show_paths::ShowPaths;
pub use show_targets::parse_show_targets;
