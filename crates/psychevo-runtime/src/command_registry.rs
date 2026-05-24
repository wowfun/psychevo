#[allow(unused_imports)]
pub(crate) use super::*;

#[path = "command_registry/specs.rs"]
mod specs;
#[allow(unused_imports)]
pub use specs::*;
#[path = "command_registry/parsing.rs"]
mod parsing;
#[allow(unused_imports)]
pub use parsing::*;
