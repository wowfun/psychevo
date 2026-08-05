#[path = "bottom_panel/agents.rs"]
mod agents;
#[path = "bottom_panel/clipboard_editor.rs"]
mod clipboard_editor;
#[path = "bottom_panel/models_sessions.rs"]
mod models_sessions;
pub(crate) use clipboard_editor::{
    agent_editor_markdown, parse_agent_editor_max_spawn_depth, strip_dotenv_quotes,
    valid_local_agent_name,
};
#[path = "bottom_panel/history_messages.rs"]
mod history_messages;
