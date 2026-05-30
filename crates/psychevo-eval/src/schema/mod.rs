#![allow(dead_code)]

#[allow(unused_imports)]
use crate::*;

pub const MANIFEST_SCHEMA_VERSION: u32 = 5;
pub const EVALUATOR_RESULT_SCHEMA_VERSION: u32 = 2;
pub const ARTIFACT_SCHEMA_VERSION: u32 = 8;
pub const TASK_ENV_SCHEMA_VERSION: u32 = 1;
pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const VIEW_SCHEMA_VERSION: u32 = 17;
pub const WORKSPACE_SCHEMA_VERSION: u32 = 2;
pub const SCHEMA_VERSION: u32 = MANIFEST_SCHEMA_VERSION;

mod config;
mod dataset;
mod manifest;
mod run;
mod view;

pub(crate) use config::*;
pub(crate) use dataset::*;
pub(crate) use manifest::*;
pub(crate) use run::*;
pub(crate) use view::*;
