use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("yaml failed: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("toml parse failed: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml serialize failed: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("agent failed: {0}")]
    Agent(#[from] psychevo_agent_core::Error),
    #[error("config failed: {0}")]
    Config(String),
    #[error("{0}")]
    Message(String),
}
