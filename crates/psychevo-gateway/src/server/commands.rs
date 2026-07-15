use super::*;

const SIDE_CONVERSATION_NO_SESSION_MESSAGE: &str = "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again.";
const SIDE_CONVERSATION_NO_TARGET_MESSAGE: &str =
    "Select an Agent target before starting a side chat.";
type GatewaySlashConfig = SharedSlashConfig;
type GatewaySlashAlias = SharedSlashAlias;
type GatewaySlashKeybind = SharedSlashKeybind;

include!("commands/execute.rs");
include!("commands/list.rs");
include!("commands/settings.rs");
include!("commands/presentation.rs");
