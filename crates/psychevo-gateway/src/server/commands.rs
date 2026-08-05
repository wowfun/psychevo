use psychevo::command_registry::{SharedSlashAlias, SharedSlashConfig, SharedSlashKeybind};

const SIDE_CONVERSATION_NO_SESSION_MESSAGE: &str = "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again.";
const SIDE_CONVERSATION_NO_TARGET_MESSAGE: &str =
    "Select an Agent target before starting a side chat.";
type GatewaySlashConfig = SharedSlashConfig;
type GatewaySlashAlias = SharedSlashAlias;
type GatewaySlashKeybind = SharedSlashKeybind;

mod execute;
mod list;
mod presentation;
mod settings;

pub(super) use execute::command_execute_value;
pub(crate) use execute::record_gateway_mission_metadata_for_parent;
pub(super) use list::{command_list_result, command_list_value};
pub(super) use presentation::{command_item_completion_detail, command_item_matches};
pub(super) use settings::{slash_settings_read_value, slash_settings_update_value};
