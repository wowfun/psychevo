use std::collections::BTreeSet;

use psychevo_agent_core::ToolExposure;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::prompt_assembly::stable_hash_hex;

pub const CAPABILITY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    Tool,
    Toolset,
    Skill,
    Agent,
    Provider,
    Context,
    Memory,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySourceRecord {
    pub id: String,
    pub kind: String,
    pub raw_identity: String,
    pub model_visible_identity: Option<String>,
    pub lifetime: String,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityContributionRecord {
    pub id: String,
    pub source_id: String,
    pub category: CapabilityCategory,
    pub raw_name: String,
    pub visible_name: Option<String>,
    pub exposure: Option<ToolExposure>,
    pub status: String,
    pub reason: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySelectionRecord {
    pub contribution_id: String,
    pub category: CapabilityCategory,
    pub visible_name: Option<String>,
    pub exposure: Option<ToolExposure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySelectionEvent {
    pub order: usize,
    pub event: String,
    pub contribution_id: Option<String>,
    pub source_id: Option<String>,
    pub category: CapabilityCategory,
    pub visible_name: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySnapshotParts {
    pub sources: Vec<CapabilitySourceRecord>,
    pub contributions: Vec<CapabilityContributionRecord>,
    pub selected: Vec<CapabilitySelectionRecord>,
    pub selection_events: Vec<CapabilitySelectionEvent>,
}

impl CapabilitySnapshotParts {
    pub fn merge(&mut self, other: CapabilitySnapshotParts) {
        for source in other.sources {
            self.push_source(source);
        }
        self.contributions.extend(other.contributions);
        self.selected.extend(other.selected);
        for mut event in other.selection_events {
            event.order = self.selection_events.len();
            self.selection_events.push(event);
        }
    }

    pub fn push_source(&mut self, source: CapabilitySourceRecord) {
        if self.sources.iter().any(|existing| existing.id == source.id) {
            return;
        }
        self.sources.push(source);
    }

    pub fn push_selected(&mut self, contribution: CapabilityContributionRecord) {
        let contribution_id = contribution.id.clone();
        let category = contribution.category.clone();
        let visible_name = contribution.visible_name.clone();
        let exposure = contribution.exposure;
        let source_id = contribution.source_id.clone();
        self.contributions.push(contribution);
        self.selected.push(CapabilitySelectionRecord {
            contribution_id: contribution_id.clone(),
            category: category.clone(),
            visible_name: visible_name.clone(),
            exposure,
        });
        self.push_event(
            "selected",
            Some(contribution_id),
            Some(source_id),
            category,
            visible_name,
            None,
        );
    }

    pub fn push_omitted(&mut self, contribution: CapabilityContributionRecord, event: &str) {
        let contribution_id = contribution.id.clone();
        let source_id = contribution.source_id.clone();
        let category = contribution.category.clone();
        let visible_name = contribution.visible_name.clone();
        let reason = contribution.reason.clone();
        self.contributions.push(contribution);
        self.push_event(
            event,
            Some(contribution_id),
            Some(source_id),
            category,
            visible_name,
            reason,
        );
    }

    pub fn push_event(
        &mut self,
        event: &str,
        contribution_id: Option<String>,
        source_id: Option<String>,
        category: CapabilityCategory,
        visible_name: Option<String>,
        reason: Option<String>,
    ) {
        self.selection_events.push(CapabilitySelectionEvent {
            order: self.selection_events.len(),
            event: event.to_string(),
            contribution_id,
            source_id,
            category,
            visible_name,
            reason,
        });
    }

    pub fn source_ids(&self) -> BTreeSet<&str> {
        self.sources
            .iter()
            .map(|source| source.id.as_str())
            .collect()
    }

    pub fn retain_selected_tools(&mut self, final_visible_names: &BTreeSet<String>, reason: &str) {
        let omitted = self
            .selected
            .iter()
            .filter(|selection| selection.category == CapabilityCategory::Tool)
            .filter_map(|selection| {
                let visible_name = selection.visible_name.as_ref()?;
                (!final_visible_names.contains(visible_name)).then(|| {
                    (
                        selection.contribution_id.clone(),
                        visible_name.clone(),
                        selection.exposure,
                    )
                })
            })
            .collect::<Vec<_>>();
        if omitted.is_empty() {
            return;
        }
        let omitted_ids = omitted
            .iter()
            .map(|(id, _, _)| id.as_str())
            .collect::<BTreeSet<_>>();
        self.selected
            .retain(|selection| !omitted_ids.contains(selection.contribution_id.as_str()));
        for (contribution_id, visible_name, _) in omitted {
            let source_id = self
                .contributions
                .iter_mut()
                .find(|contribution| contribution.id == contribution_id)
                .map(|contribution| {
                    let source_id = contribution.source_id.clone();
                    contribution.status = "omitted".to_string();
                    contribution.reason = Some(reason.to_string());
                    source_id
                });
            if let Some(source_id) = source_id {
                self.push_event(
                    "omitted",
                    Some(contribution_id),
                    Some(source_id),
                    CapabilityCategory::Tool,
                    Some(visible_name),
                    Some(reason.to_string()),
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySnapshot {
    pub schema_version: u32,
    pub session_id: String,
    pub prompt_prefix_version: i64,
    pub created_at_ms: i64,
    pub snapshot_hash: String,
    pub sources: Vec<CapabilitySourceRecord>,
    pub contributions: Vec<CapabilityContributionRecord>,
    pub selected: Vec<CapabilitySelectionRecord>,
    pub selection_events: Vec<CapabilitySelectionEvent>,
}

impl CapabilitySnapshot {
    pub fn from_parts(
        session_id: String,
        prompt_prefix_version: i64,
        created_at_ms: i64,
        mut parts: CapabilitySnapshotParts,
    ) -> Self {
        parts.sources.sort_by(|left, right| left.id.cmp(&right.id));
        parts
            .contributions
            .sort_by(|left, right| left.id.cmp(&right.id));
        parts
            .selected
            .sort_by(|left, right| left.contribution_id.cmp(&right.contribution_id));
        let mut snapshot = Self {
            schema_version: CAPABILITY_SNAPSHOT_SCHEMA_VERSION,
            session_id,
            prompt_prefix_version,
            created_at_ms,
            snapshot_hash: String::new(),
            sources: parts.sources,
            contributions: parts.contributions,
            selected: parts.selected,
            selection_events: parts.selection_events,
        };
        snapshot.snapshot_hash = snapshot.compute_hash();
        snapshot
    }

    fn compute_hash(&self) -> String {
        let canonical = json!({
            "schema_version": self.schema_version,
            "session_id": self.session_id,
            "prompt_prefix_version": self.prompt_prefix_version,
            "sources": self.sources,
            "contributions": self.contributions,
            "selected": self.selected,
            "selection_events": self.selection_events,
        });
        stable_hash_hex(&serde_json::to_string(&canonical).unwrap_or_default())
    }
}

pub(crate) fn source_record(
    id: impl Into<String>,
    kind: impl Into<String>,
    raw_identity: impl Into<String>,
    model_visible_identity: Option<String>,
    lifetime: impl Into<String>,
    provenance: Option<String>,
) -> CapabilitySourceRecord {
    CapabilitySourceRecord {
        id: id.into(),
        kind: kind.into(),
        raw_identity: raw_identity.into(),
        model_visible_identity,
        lifetime: lifetime.into(),
        provenance,
    }
}
