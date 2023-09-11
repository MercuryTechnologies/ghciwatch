//! Parsers for `ghci` output.

mod haskell_grammar;
mod module_and_files;
mod module_set;

use haskell_grammar::module_name;

pub use module_and_files::Module;
pub use module_set::ModuleSet;
