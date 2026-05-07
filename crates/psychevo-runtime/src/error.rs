use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent failed: {0}")]
    Agent(#[from] psychevo_agent_core::Error),
    #[error("config failed: {0}")]
    Config(String),
    #[error("{0}")]
    Message(String),
}
