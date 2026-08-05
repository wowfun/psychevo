use std::collections::HashSet;

use psychevo::{Error, application::GatewaySourceLaneInput};
use serde_json::{Value, json};

use super::Gateway;
use super::stream_input::{source_key_key, thread_key};
use psychevo_gateway_protocol::source::{
    GatewayBackendInfo, GatewaySource, GatewaySourceLifetime, SourceKey,
};

impl Gateway {
    pub async fn reset_source(
        &self,
        source: &GatewaySource,
        new_thread_id: &str,
    ) -> psychevo::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot reset invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.restore_framework_thread(new_thread_id).await?;
                let previous = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), new_thread_id.to_string());
                if let Some(previous) = previous {
                    self.archive_framework_thread(&previous, "gateway_reset")
                        .await?;
                }
            }
            GatewaySourceLifetime::Persistent => {
                self.restore_framework_thread(new_thread_id).await?;
                let previous_thread_id = self
                    .durability
                    .gateway_source_lane(&source_key.0)
                    .await?
                    .and_then(|previous| previous.thread_id);
                self.durability
                    .upsert_gateway_source_lane(GatewaySourceLaneInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: Some(new_thread_id),
                        draft_agent_ref: None,
                        draft_profile_ref: None,
                        draft_control_values: &Default::default(),
                        lineage: Some(json!({"reason": "gateway_reset"})),
                    })
                    .await?;
                if let Some(previous_thread_id) = previous_thread_id
                    && previous_thread_id != new_thread_id
                {
                    self.archive_framework_thread(&previous_thread_id, "gateway_reset")
                        .await?;
                }
            }
        }
        self.bump_source_generation_key(&source_key);
        Ok(())
    }

    pub async fn clear_source_binding(
        &self,
        source: &GatewaySource,
    ) -> psychevo::Result<Option<String>> {
        let source_key = source.source_key();
        let previous = match source.lifetime {
            GatewaySourceLifetime::Invocation => return Ok(None),
            GatewaySourceLifetime::Process => self
                .process_bindings
                .lock()
                .expect("gateway process binding map poisoned")
                .remove(&source_key.0),
            GatewaySourceLifetime::Persistent => {
                let previous = self
                    .durability
                    .gateway_source_lane(&source_key.0)
                    .await?
                    .and_then(|lane| lane.thread_id);
                self.durability
                    .delete_gateway_source_binding(&source_key.0)
                    .await?;
                previous
            }
        };
        self.bump_source_generation_key(&source_key);
        Ok(previous)
    }

    pub async fn reset_source_to_empty(
        &self,
        source: &GatewaySource,
    ) -> psychevo::Result<Option<String>> {
        let previous = self.clear_source_binding(source).await?;
        if let Some(previous) = previous.as_deref() {
            self.archive_framework_thread(previous, "gateway_reset")
                .await?;
        }
        Ok(previous)
    }

    pub async fn rotate_channel_connection_sources(
        &self,
        connection_id: &str,
    ) -> psychevo::Result<usize> {
        let bindings = self
            .durability
            .gateway_source_bindings_for_connection_id(connection_id)
            .await?;
        let mut rotated = 0usize;
        let mut archived_threads = HashSet::new();
        for binding in bindings {
            if !self
                .durability
                .delete_gateway_source_binding(&binding.source_key)
                .await?
            {
                continue;
            }

            rotated += 1;
            let source_key = SourceKey(binding.source_key.clone());
            self.bump_source_generation_key(&source_key);
            self.register_active_queue_alias(
                &source_key_key(&source_key),
                &thread_key(&binding.thread_id),
            );

            if archived_threads.insert(binding.thread_id.clone()) {
                self.archive_framework_thread(&binding.thread_id, "channel_workspace_changed")
                    .await?;
            }
        }
        Ok(rotated)
    }

    pub async fn bind_source_thread(
        &self,
        source: &GatewaySource,
        thread_id: &str,
        backend: &GatewayBackendInfo,
        lineage: Option<Value>,
    ) -> psychevo::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot bind invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), thread_id.to_string());
            }
            GatewaySourceLifetime::Persistent => {
                self.durability
                    .upsert_gateway_source_lane(GatewaySourceLaneInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: Some(thread_id),
                        draft_agent_ref: None,
                        draft_profile_ref: None,
                        draft_control_values: &Default::default(),
                        lineage: lineage_with_runtime_ref(lineage, backend.runtime_ref.as_deref()),
                    })
                    .await?;
            }
        }
        self.bump_source_generation_key(&source_key);
        Ok(())
    }

    async fn restore_framework_thread(&self, thread_id: &str) -> psychevo::Result<()> {
        self.framework_client()
            .resume_thread(thread_id)
            .await?
            .restore()
            .await
    }

    async fn archive_framework_thread(
        &self,
        thread_id: &str,
        reason: &'static str,
    ) -> psychevo::Result<()> {
        self.framework_client()
            .resume_thread(thread_id)
            .await?
            .archive_with_reason(reason)
            .await
    }
}

fn lineage_with_runtime_ref(
    mut lineage: Option<Value>,
    runtime_ref: Option<&str>,
) -> Option<Value> {
    let runtime_ref = runtime_ref?;
    let value = lineage.get_or_insert_with(|| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("runtimeRef".to_string(), json!(runtime_ref));
    }
    lineage
}
