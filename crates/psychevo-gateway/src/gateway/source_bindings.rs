impl Gateway {
    pub fn reset_source(
        &self,
        source: &GatewaySource,
        new_thread_id: &str,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot reset invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.state.resume_session(new_thread_id)?;
                let previous = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), new_thread_id.to_string());
                if let Some(previous) = previous {
                    self.state

                        .mark_session_ended_with_reason(&previous, "gateway_reset")?;
                    self.state.archive_session(&previous)?;
                }
            }
            GatewaySourceLifetime::Persistent => {
                if let Some(previous) = self.state.gateway_source_lane(&source_key.0)?
                    && let Some(previous_thread_id) = previous.thread_id
                {
                    self.state

                        .mark_session_ended_with_reason(&previous_thread_id, "gateway_reset")?;
                    self.state.archive_session(&previous_thread_id)?;
                }
                self.state

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
                    })?;
            }
        }
        self.bump_source_generation_key(&source_key);
        Ok(())
    }

    pub fn clear_source_binding(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
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
                    .state

                    .gateway_source_lane(&source_key.0)?
                    .and_then(|lane| lane.thread_id);
                self.state

                    .delete_gateway_source_binding(&source_key.0)?;
                previous
            }
        };
        self.bump_source_generation_key(&source_key);
        Ok(previous)
    }

    pub fn reset_source_to_empty(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        let previous = self.clear_source_binding(source)?;
        if let Some(previous) = previous.as_deref() {
            self.state

                .mark_session_ended_with_reason(previous, "gateway_reset")?;
            self.state.archive_session(previous)?;
        }
        Ok(previous)
    }

    pub fn rotate_channel_connection_sources(
        &self,
        connection_id: &str,
    ) -> psychevo_runtime::Result<usize> {
        let bindings = self
            .state

            .gateway_source_bindings_for_connection_id(connection_id)?;
        let mut rotated = 0usize;
        let mut archived_threads = HashSet::new();
        for binding in bindings {
            if !self
                .state

                .delete_gateway_source_binding(&binding.source_key)?
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
                self.state.mark_session_ended_with_reason(
                    &binding.thread_id,
                    "channel_workspace_changed",
                )?;
                self.state.archive_session(&binding.thread_id)?;
            }
        }
        Ok(rotated)
    }

    pub fn bind_source_thread(
        &self,
        source: &GatewaySource,
        thread_id: &str,
        backend: &GatewayBackendInfo,
        lineage: Option<Value>,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {
                return Err(Error::Message(
                    "cannot bind invocation-scoped gateway source".to_string(),
                ));
            }
            GatewaySourceLifetime::Process => {
                self.state.resume_session(thread_id)?;
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), thread_id.to_string());
            }
            GatewaySourceLifetime::Persistent => {
                self.state

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
                    })?;
            }
        }
        self.bump_source_generation_key(&source_key);
        Ok(())
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
