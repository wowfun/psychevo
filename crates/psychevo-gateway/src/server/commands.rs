use super::*;

const SIDE_CONVERSATION_NO_SESSION_MESSAGE: &str = "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again.";
type GatewaySlashConfig = SharedSlashConfig;
type GatewaySlashAlias = SharedSlashAlias;
type GatewaySlashKeybind = SharedSlashKeybind;

include!("commands/execute.rs");
include!("commands/list.rs");
include!("commands/settings.rs");
include!("commands/presentation.rs");
