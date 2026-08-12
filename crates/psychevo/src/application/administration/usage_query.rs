use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct UsageQuery {
    pub cwd: PathBuf,
    pub all: bool,
    pub days: Option<u64>,
    pub limit: usize,
}

impl UsageQuery {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            all: false,
            days: None,
            limit: 20,
        }
    }
}
