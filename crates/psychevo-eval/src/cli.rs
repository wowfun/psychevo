#[allow(unused_imports)]
use crate::*;

mod args;
mod commands;
mod dispatch;
mod parsing;
mod util;

pub use args::CliOutcome;
pub use dispatch::run_cli_from;

pub(crate) use args::*;
pub(crate) use commands::*;
pub(crate) use dispatch::*;
pub(crate) use parsing::*;
pub(crate) use util::*;

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    pub(crate) mod project_lifecycle;
}
