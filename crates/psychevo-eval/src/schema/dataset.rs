#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub struct RunSelectorFilters {
    pub task_set: Option<String>,
    pub agent: Option<String>,
    pub status: Option<RunStatusFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEntry {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    pub payload_exists: bool,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DatasetImportRequest {
    pub store_root: Option<PathBuf>,
    pub path: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub loader: Option<String>,
    pub split: Option<String>,
    pub sample_limit: Option<usize>,
    pub cache_key: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}
