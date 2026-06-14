#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionExportFormat {
    Markdown,
    Json,
}

impl SessionExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionArtifactKind {
    Export,
    Share,
}

impl SessionArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Share => "share",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionExportInclude {
    Header,
    Messages,
    Reasoning,
    ProviderInputEvidence,
    LastProviderRequest,
    LastProviderResponse,
}

impl SessionExportInclude {
    pub fn parse_token(value: &str) -> Option<Self> {
        match value {
            "header" | "h" => Some(Self::Header),
            "messages" | "m" => Some(Self::Messages),
            "reasoning" | "r" => Some(Self::Reasoning),
            "provider-input-evidence" | "pie" => Some(Self::ProviderInputEvidence),
            "last-provider-request" | "lpr" => Some(Self::LastProviderRequest),
            "last-provider-response" => Some(Self::LastProviderResponse),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Messages => "messages",
            Self::Reasoning => "reasoning",
            Self::ProviderInputEvidence => "provider-input-evidence",
            Self::LastProviderRequest => "last-provider-request",
            Self::LastProviderResponse => "last-provider-response",
        }
    }
}

impl Serialize for SessionExportInclude {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExportIncludeSet {
    pub(crate) values: BTreeSet<SessionExportInclude>,
}

impl SessionExportIncludeSet {
    pub fn default_for(_artifact_kind: SessionArtifactKind) -> Self {
        Self::from_values([SessionExportInclude::Messages])
    }

    pub fn parse(value: &str, artifact_kind: SessionArtifactKind) -> Result<Self> {
        let mut values = Vec::new();
        for token in value
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            let include = SessionExportInclude::parse_token(token).ok_or_else(|| {
                Error::Message(format!(
                    "unknown export include `{token}`; expected comma-separated values from {}",
                    include_usage_for_artifact(artifact_kind)
                ))
            })?;
            values.push(include);
        }
        if values.is_empty() {
            return Err(Error::Message(format!(
                "empty export include list; expected comma-separated values from {}",
                include_usage_for_artifact(artifact_kind)
            )));
        }
        Self::new(values, artifact_kind)
    }

    pub fn new(
        values: impl IntoIterator<Item = SessionExportInclude>,
        artifact_kind: SessionArtifactKind,
    ) -> Result<Self> {
        let mut set = Self::from_values(values);
        set.expand_dependencies();
        set.validate_for_artifact(artifact_kind)?;
        Ok(set)
    }

    pub fn contains(&self, include: SessionExportInclude) -> bool {
        self.values.contains(&include)
    }

    pub fn values(&self) -> impl Iterator<Item = SessionExportInclude> + '_ {
        self.values.iter().copied()
    }

    pub fn tokens(&self) -> Vec<&'static str> {
        self.values().map(SessionExportInclude::as_str).collect()
    }

    pub(crate) fn from_values(values: impl IntoIterator<Item = SessionExportInclude>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }

    pub(crate) fn expand_dependencies(&mut self) {
        if self.contains(SessionExportInclude::Reasoning) {
            self.values.insert(SessionExportInclude::Messages);
        }
    }

    pub(crate) fn validate_for_artifact(&self, artifact_kind: SessionArtifactKind) -> Result<()> {
        if artifact_kind == SessionArtifactKind::Share
            && self.contains(SessionExportInclude::LastProviderRequest)
        {
            return Err(Error::Message(
                "share artifacts do not support include value `last-provider-request`".to_string(),
            ));
        }
        if artifact_kind == SessionArtifactKind::Share
            && self.contains(SessionExportInclude::LastProviderResponse)
        {
            return Err(Error::Message(
                "share artifacts do not support include value `last-provider-response`".to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn include_usage_for_artifact(artifact_kind: SessionArtifactKind) -> &'static str {
    match artifact_kind {
        SessionArtifactKind::Export => {
            "header,messages,reasoning,provider-input-evidence,last-provider-request,last-provider-response"
        }
        SessionArtifactKind::Share => "header,messages,reasoning,provider-input-evidence",
    }
}

#[derive(Debug, Clone)]
pub struct SessionExportOptions {
    pub format: SessionExportFormat,
    pub include: SessionExportIncludeSet,
    pub artifact_kind: SessionArtifactKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionExportArtifact {
    pub content: String,
    pub format: SessionExportFormat,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionExportWriteResult {
    pub path: PathBuf,
    pub bytes: usize,
    pub format: SessionExportFormat,
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExportMessageRecord {
    pub(crate) session_seq: i64,
    pub(crate) message: Message,
    pub(crate) usage: Option<Value>,
    pub(crate) metadata: Option<Value>,
}
