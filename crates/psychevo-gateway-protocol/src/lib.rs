use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

include!("protocol/source.rs");
include!("protocol/events_transcript.rs");
include!("protocol/thread_command_turn.rs");
include!("protocol/channels.rs");
include!("protocol/settings_workspace_context.rs");
include!("protocol/agents_backend_rpc.rs");
include!("protocol/codegen.rs");
