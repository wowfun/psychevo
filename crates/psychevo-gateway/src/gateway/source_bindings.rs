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
                self.state.store().resume_session(new_thread_id)?;
                let previous = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), new_thread_id.to_string());
                if let Some(previous) = previous {
                    self.state
                        .store()
                        .mark_session_ended_with_reason(&previous, "gateway_reset")?;
                    self.state.store().archive_session(&previous)?;
                }
            }
            GatewaySourceLifetime::Persistent => {
                if let Some(previous) = self.state.store().gateway_source_binding(&source_key.0)? {
                    self.state
                        .store()
                        .mark_session_ended_with_reason(&previous.thread_id, "gateway_reset")?;
                    self.state.store().archive_session(&previous.thread_id)?;
                }
                self.state
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: new_thread_id,
                        backend_kind: self.backend.kind().as_str(),
                        backend_native_id: Some(new_thread_id),
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
                    .store()
                    .gateway_source_binding(&source_key.0)?
                    .map(|binding| binding.thread_id);
                self.state
                    .store()
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
                .store()
                .mark_session_ended_with_reason(previous, "gateway_reset")?;
            self.state.store().archive_session(previous)?;
        }
        Ok(previous)
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
                self.state.store().resume_session(thread_id)?;
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), thread_id.to_string());
            }
            GatewaySourceLifetime::Persistent => {
                self.state
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id,
                        backend_kind: backend.kind.as_str(),
                        backend_native_id: backend.native_id.as_deref(),
                        lineage,
                    })?;
            }
        }
        self.bump_source_generation_key(&source_key);
        Ok(())
    }

}
