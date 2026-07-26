use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContributionProjection {
    facts: Vec<ContributionFact>,
}

impl ContributionProjection {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(&mut self, fact: ContributionFact) {
        self.facts.push(fact);
    }

    pub(crate) fn extend(&mut self, other: ContributionProjection) {
        self.facts.extend(other.facts);
    }

    pub(crate) fn facts(&self) -> &[ContributionFact] {
        &self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContributionFact {
    pub(crate) source_id: String,
    pub(crate) source_kind: String,
    pub(crate) declaration_family: String,
    pub(crate) owner_module: String,
    pub(crate) effect_target: String,
    pub(crate) status: ContributionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

impl ContributionFact {
    pub(crate) fn new(
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
        declaration_family: impl Into<String>,
        owner_module: impl Into<String>,
        effect_target: impl Into<String>,
        status: ContributionStatus,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            source_kind: source_kind.into(),
            declaration_family: declaration_family.into(),
            owner_module: owner_module.into(),
            effect_target: effect_target.into(),
            status,
            reason: None,
        }
    }

    pub(crate) fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContributionStatus {
    Accepted,
    Omitted,
    Unsupported,
    Unavailable,
    Degraded,
    Conflict,
    Hidden,
    Invalid,
}
