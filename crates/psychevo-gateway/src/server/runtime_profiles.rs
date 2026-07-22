use super::*;

#[derive(Clone)]
struct RuntimeProfileRecord {
    config: RuntimeProfileConfig,
    generated: bool,
}

pub(super) struct RunnableTargetCatalog {
    profile_records: BTreeMap<String, RuntimeProfileRecord>,
    profile_views: Vec<wire::RuntimeProfileView>,
    compatible_targets: Vec<wire::RunnableTargetView>,
    target_revisions: BTreeMap<String, String>,
}

pub(super) struct ThreadDraftPrepareWork {
    pub(super) target_catalog: Arc<RunnableTargetCatalog>,
    pub(super) target: wire::RunnableTargetView,
    pub(super) context: wire::ThreadContextReadResult,
    pub(super) configured: Vec<psychevo_runtime::ConfiguredModel>,
    pub(super) source_lane_prepared: bool,
}

#[derive(Clone)]
pub(super) struct ImportableAcpProfile {
    pub(super) config: RuntimeProfileConfig,
    pub(super) view: wire::RuntimeProfileView,
    pub(super) targets: Vec<wire::RunnableTargetView>,
}

pub(super) fn importable_acp_profiles(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Vec<ImportableAcpProfile>> {
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    let views = catalog
        .profile_views
        .iter()
        .map(|view| (view.id.as_str(), view))
        .collect::<BTreeMap<_, _>>();
    let mut profiles = catalog
        .profile_records
        .values()
        .filter(|record| record.config.runtime == RuntimeProfileKind::Acp && record.config.enabled)
        .filter_map(|record| {
            let view = views.get(record.config.id.as_str())?;
            let targets = catalog
                .compatible_targets
                .iter()
                .filter(|target| target.runtime_profile_ref == record.config.id)
                .cloned()
                .map(|mut target| {
                    if target.ready
                        && let Err(error) = resolve_runtime_target_peer_turn(
                            state,
                            scope,
                            &record.config.id,
                            target.agent_ref.as_deref(),
                        )
                    {
                        target.ready = false;
                        target.unavailable_reason = Some(error.to_string());
                    }
                    target
                })
                .collect::<Vec<_>>();
            Some(ImportableAcpProfile {
                config: record.config.clone(),
                view: (*view).clone(),
                targets,
            })
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| left.view.label.cmp(&right.view.label));
    Ok(profiles)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidatedRunnableTarget {
    pub(super) agent_ref: Option<String>,
    pub(super) runtime_profile_ref: String,
    pub(super) backend_kind: wire::BackendKind,
}

impl RunnableTargetCatalog {
    pub(super) fn load(
        state: &WebState,
        scope: &ResolvedScope,
    ) -> psychevo_runtime::Result<Arc<Self>> {
        let generation = state
            .inner
            .runnable_target_catalog_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if let Some((_, catalog)) = state
            .inner
            .runnable_target_catalogs
            .lock()
            .expect("runnable target catalogs poisoned")
            .get(&scope.cwd)
            .filter(|(cached_generation, _)| *cached_generation == generation)
        {
            return Ok(catalog.clone());
        }
        let catalog = Arc::new(Self::build(state, scope)?);
        let current_generation = state
            .inner
            .runnable_target_catalog_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if current_generation == generation {
            state
                .inner
                .runnable_target_catalogs
                .lock()
                .expect("runnable target catalogs poisoned")
                .insert(scope.cwd.clone(), (generation, catalog.clone()));
        }
        Ok(catalog)
    }

    fn build(state: &WebState, scope: &ResolvedScope) -> psychevo_runtime::Result<Self> {
        // Thread Context is a cache-only read. Discover from the effective
        // config without materializing, probing, or launching a backend.
        let agents = discover_agents(&AgentDiscoveryOptions {
            home: state.inner.home.clone(),
            cwd: scope.cwd.clone(),
            env: state.inner.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })?;
        let profile_records = runtime_profile_records(state, scope)?;
        let backends =
            load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
        let profile_views = profile_records
            .values()
            .map(|record| runtime_profile_view_with_backends(state, scope, record, None, &backends))
            .collect::<psychevo_runtime::Result<Vec<_>>>()?;
        let (compatible_targets, target_revisions) =
            compatible_runnable_targets(&profile_records, &profile_views, &agents, &backends);
        Ok(Self {
            profile_records,
            profile_views,
            compatible_targets,
            target_revisions,
        })
    }

    fn compatible_pair(
        &self,
        target: &wire::RunnableTargetInput,
    ) -> psychevo_runtime::Result<&wire::RunnableTargetView> {
        let runtime_profile_ref = target.runtime_profile_ref.trim();
        if runtime_profile_ref.is_empty() {
            return Err(agent_session_error(
                "invalid_target",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "RunnableTarget.runtimeProfileRef must be non-empty.",
                None,
            ));
        }
        let agent_ref = match target.agent_ref.as_deref() {
            Some(agent_ref) if agent_ref.trim().is_empty() => {
                return Err(agent_session_error(
                    "invalid_target",
                    AgentErrorStage::Binding,
                    "user_action",
                    "not_delivered",
                    "RunnableTarget.agentRef must be null for the default Agent or a non-empty Agent Definition id.",
                    None,
                ));
            }
            Some(agent_ref) => Some(agent_ref.trim()),
            None => None,
        };
        self.compatible_targets
            .iter()
            .find(|candidate| {
                candidate.runtime_profile_ref == runtime_profile_ref
                    && candidate.agent_ref.as_deref() == agent_ref
            })
            .ok_or_else(|| {
                let agent_label = agent_ref.unwrap_or("Default Agent");
                agent_session_error(
                    "incompatible_target",
                    AgentErrorStage::Binding,
                    "user_action",
                    "not_delivered",
                    format!(
                        "Agent target `{agent_label}` is not compatible with Runtime Profile `{runtime_profile_ref}`. Refresh Thread Context and select one of its compatibleTargets."
                    ),
                    None,
                )
            })
    }

    pub(super) fn by_id(&self, target_id: &str) -> Option<&wire::RunnableTargetView> {
        self.compatible_targets
            .iter()
            .find(|candidate| candidate.target_id == target_id)
    }

    pub(super) fn default_draft_target(
        &self,
        state: &WebState,
        scope: &ResolvedScope,
    ) -> psychevo_runtime::Result<wire::RunnableTargetView> {
        let source_lane = state
            .inner
            .state
            .store()
            .gateway_source_lane(&scope.source.source_key().0)?;
        if let Some(lane) = source_lane.as_ref()
            && let Some(runtime_profile_ref) = lane.draft_profile_ref.as_deref()
        {
            return self
                .compatible_pair(&wire::RunnableTargetInput {
                    agent_ref: lane.draft_agent_ref.clone(),
                    runtime_profile_ref: runtime_profile_ref.to_string(),
                })
                .cloned();
        }
        self.compatible_targets
            .iter()
            .find(|target| target.agent_ref.is_none() && target.runtime_profile_ref == "native")
            .or_else(|| self.compatible_targets.iter().find(|target| target.ready))
            .or_else(|| self.compatible_targets.first())
            .cloned()
            .ok_or_else(|| Error::Message("No default Agent target is available.".to_string()))
    }

    fn target_revision(&self, target_id: &str) -> Option<&str> {
        self.target_revisions.get(target_id).map(String::as_str)
    }

    fn validate(
        &self,
        target: &wire::RunnableTargetInput,
    ) -> psychevo_runtime::Result<ValidatedRunnableTarget> {
        let compatible = self.compatible_pair(target)?;
        if !compatible.ready {
            return Err(agent_session_error(
                "target_unavailable",
                AgentErrorStage::Configuration,
                "user_action",
                "not_delivered",
                compatible.unavailable_reason.clone().unwrap_or_else(|| {
                    format!(
                        "RunnableTarget for Runtime Profile `{}` is unavailable.",
                        compatible.runtime_profile_ref
                    )
                }),
                None,
            ));
        }
        Ok(ValidatedRunnableTarget {
            agent_ref: compatible.agent_ref.clone(),
            runtime_profile_ref: compatible.runtime_profile_ref.clone(),
            backend_kind: match self
                .profile_records
                .get(&compatible.runtime_profile_ref)
                .expect("compatible target references a loaded Runtime Profile")
                .config
                .runtime
            {
                RuntimeProfileKind::Native => wire::BackendKind::Native,
                RuntimeProfileKind::Acp => wire::BackendKind::Acp,
            },
        })
    }
}

impl WebState {
    pub(super) fn invalidate_runnable_target_catalog(&self) {
        self.inner
            .runnable_target_catalog_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.inner
            .runnable_target_catalogs
            .lock()
            .expect("runnable target catalogs poisoned")
            .clear();
    }

    pub(super) fn invalidate_runnable_target_catalog_after<T>(
        &self,
        result: psychevo_runtime::Result<T>,
    ) -> psychevo_runtime::Result<T> {
        if result.is_ok() {
            self.invalidate_runnable_target_catalog();
        }
        result
    }
}

pub(super) fn runnable_target_input(
    target: &wire::RunnableTargetView,
) -> wire::RunnableTargetInput {
    wire::RunnableTargetInput {
        agent_ref: target.agent_ref.clone(),
        runtime_profile_ref: target.runtime_profile_ref.clone(),
    }
}

pub(super) fn runnable_target_by_id(
    state: &WebState,
    scope: &ResolvedScope,
    target_id: &str,
) -> psychevo_runtime::Result<wire::RunnableTargetView> {
    RunnableTargetCatalog::load(state, scope)?
        .by_id(target_id)
        .cloned()
        .ok_or_else(|| {
            agent_session_error(
                "target_not_found",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "The selected Agent target is no longer present in this workspace catalog. Refresh Thread Context and select another target.",
                None,
            )
        })
}

/// Resolves the source lane's single canonical Agent target. Callers provide
/// only the connection default; immutable Thread bindings and source drafts
/// remain owned by the catalog/application boundary.
pub(super) fn runnable_target_for_source(
    state: &WebState,
    scope: &ResolvedScope,
    source: &GatewaySource,
    default_runtime_profile_ref: &str,
) -> psychevo_runtime::Result<wire::RunnableTargetView> {
    resolve_runnable_target_for_source(state, scope, source, None, default_runtime_profile_ref)
}

pub(super) fn runnable_target_for_source_profile(
    state: &WebState,
    scope: &ResolvedScope,
    source: &GatewaySource,
    requested_runtime_profile_ref: Option<&str>,
) -> psychevo_runtime::Result<wire::RunnableTargetView> {
    resolve_runnable_target_for_source(
        state,
        scope,
        source,
        requested_runtime_profile_ref,
        "native",
    )
}

fn resolve_runnable_target_for_source(
    state: &WebState,
    scope: &ResolvedScope,
    source: &GatewaySource,
    requested_runtime_profile_ref: Option<&str>,
    default_runtime_profile_ref: &str,
) -> psychevo_runtime::Result<wire::RunnableTargetView> {
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source.source_key().0)?;
    let binding = lane
        .as_ref()
        .and_then(|lane| lane.thread_id.as_deref())
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten();
    let agent_ref = binding
        .as_ref()
        .and_then(|binding| binding.agent_ref.clone())
        .or_else(|| lane.as_ref().and_then(|lane| lane.draft_agent_ref.clone()));
    let runtime_profile_ref = requested_runtime_profile_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            binding
                .as_ref()
                .and_then(|binding| binding.runtime_ref.clone())
        })
        .or_else(|| {
            lane.as_ref()
                .and_then(|lane| lane.draft_profile_ref.clone())
        })
        .unwrap_or_else(|| default_runtime_profile_ref.to_string());
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    let requested = wire::RunnableTargetInput {
        agent_ref,
        runtime_profile_ref: runtime_profile_ref.clone(),
    };
    if let Ok(target) = catalog.compatible_pair(&requested) {
        return Ok(target.clone());
    }
    if requested.agent_ref.is_none()
        && let Some(profile) = catalog.profile_records.get(&runtime_profile_ref)
    {
        for preferred_agent_ref in [
            profile.config.default_agent.as_deref(),
            profile.config.backend_ref.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(target) = catalog.compatible_targets.iter().find(|target| {
                target.runtime_profile_ref == runtime_profile_ref
                    && target.agent_ref.as_deref() == Some(preferred_agent_ref)
            }) {
                return Ok(target.clone());
            }
        }
    }
    Ok(catalog.compatible_pair(&requested)?.clone())
}

pub(super) async fn thread_context_read_result_for_target_id(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    target_id: &str,
) -> psychevo_runtime::Result<wire::ThreadContextReadResult> {
    let has_binding = thread_id
        .as_deref()
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten()
        .is_some();
    let target = (!has_binding)
        .then(|| runnable_target_by_id(state, scope, target_id))
        .transpose()?
        .map(|target| runnable_target_input(&target));
    thread_context_read_result_live(
        state,
        scope,
        wire::ThreadContextReadParams {
            thread_id,
            target,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await
}

pub(super) fn runtime_profile_list_result(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<wire::RuntimeProfileListResult> {
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    Ok(wire::RuntimeProfileListResult {
        profiles: catalog.profile_views.clone(),
    })
}

pub(super) fn runtime_profile_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileReadParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("unknown runtime profile: {}", params.id)))?;
    Ok(serde_json::to_value(wire::RuntimeProfileReadResult {
        profile: runtime_profile_view(state, scope, record, None)?,
        options: Some(record.config.options.clone()),
    })?)
}

pub(super) fn thread_context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
) -> psychevo_runtime::Result<wire::ThreadContextReadResult> {
    thread_context_read_result_with_configured_models(state, scope, params)
        .map(|(context, _)| context)
}

fn thread_context_read_result_with_configured_models(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
) -> psychevo_runtime::Result<(
    wire::ThreadContextReadResult,
    Vec<psychevo_runtime::ConfiguredModel>,
)> {
    let target_catalog = RunnableTargetCatalog::load(state, scope)?;
    thread_context_read_result_with_catalog(state, scope, params, target_catalog)
}

fn thread_context_read_result_with_catalog(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
    target_catalog: Arc<RunnableTargetCatalog>,
) -> psychevo_runtime::Result<(
    wire::ThreadContextReadResult,
    Vec<psychevo_runtime::ConfiguredModel>,
)> {
    let requested_target = params.target.clone();
    let thread_id = match params.thread_id {
        Some(thread_id) => Some(thread_id),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let binding = thread_id
        .as_deref()
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten();
    let run_options = state.run_options(scope.cwd.clone(), thread_id.clone());
    let configured = configured_models(&run_options).unwrap_or_default();
    if let Some(binding) = binding.as_ref()
        && binding.status == GatewayRuntimeBindingStatus::Unresolved
    {
        return Err(Error::structured(
            "This thread has an unresolved runtime binding.",
            serde_json::to_value(wire::RuntimeErrorView {
                code: "unresolved_binding".to_string(),
                stage: "binding".to_string(),
                retry_class: wire::RuntimeRetryClassView::UserAction,
                message: binding
                    .unresolved_reason
                    .clone()
                    .unwrap_or_else(|| "Select a Runtime Profile explicitly.".to_string()),
                diagnostic_ref: Some(format!("runtime-binding:{}", binding.thread_id)),
            })?,
        ));
    }
    if let Some(binding) = binding.as_ref() {
        validate_bound_agent_snapshot(binding)?;
    }
    let bound_profile_record = binding
        .as_ref()
        .map(bound_runtime_profile_record)
        .transpose()?;
    let source_lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&scope.source.source_key().0)?;
    let requested_target_view = requested_target
        .as_ref()
        .map(|target| target_catalog.compatible_pair(target).cloned())
        .transpose()?;
    if let (Some(binding), Some(requested)) = (binding.as_ref(), requested_target_view.as_ref())
        && (binding.runtime_ref.as_deref() != Some(requested.runtime_profile_ref.as_str())
            || binding.agent_ref != requested.agent_ref)
    {
        return Err(runtime_rpc_error(
            "immutable_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            format!(
                "Thread `{}` is bound to Agent target `{}` with Runtime Profile `{}`; start a new Thread to use `{}`.",
                binding.thread_id,
                binding.agent_ref.as_deref().unwrap_or("Default Agent"),
                binding.runtime_ref.as_deref().unwrap_or("unresolved"),
                requested.label,
            ),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let profile_records = &target_catalog.profile_records;
    let mut profiles = target_catalog.profile_views.clone();
    if let Some(record) = bound_profile_record.as_ref() {
        let mut bound_view = runtime_profile_view(state, scope, record, None)?;
        // This row describes the immutable effective Profile captured by the
        // binding, not whichever mutable config source currently owns the id.
        bound_view.source_targets.clear();
        if let Some(index) = profiles
            .iter()
            .position(|profile| profile.id == bound_view.id)
        {
            profiles[index] = bound_view;
        } else {
            profiles.push(bound_view);
        }
    }
    let draft_target_view = source_lane
        .as_ref()
        .and_then(|lane| {
            lane.draft_profile_ref
                .as_ref()
                .map(|runtime_ref| (lane, runtime_ref))
        })
        .map(|(lane, runtime_ref)| {
            target_catalog
                .compatible_pair(&wire::RunnableTargetInput {
                    agent_ref: lane.draft_agent_ref.clone(),
                    runtime_profile_ref: runtime_ref.clone(),
                })
                .cloned()
        })
        .transpose()?;
    let bound_target_view = binding.as_ref().and_then(|binding| {
        let runtime_profile_ref = binding.runtime_ref.as_deref().unwrap_or("unresolved");
        target_catalog
            .compatible_targets
            .iter()
            .find(|target| {
                target.runtime_profile_ref == runtime_profile_ref
                    && target.agent_ref == binding.agent_ref
            })
            .cloned()
            .or_else(|| {
                profiles
                    .iter()
                    .find(|profile| profile.id == runtime_profile_ref)
                    .map(|profile| {
                        let ready = matches!(profile.health.status.as_str(), "ready" | "unchecked");
                        runnable_target_view(
                            binding.agent_ref.clone(),
                            binding.agent_ref.as_deref().unwrap_or("Psychevo"),
                            profile,
                            ready,
                            (!ready).then(|| profile.health.summary.clone()),
                        )
                    })
            })
    });
    let (selected_target, selection_state, explicit_selection) =
        if let Some(target) = bound_target_view {
            (target, "bound", true)
        } else if let Some(target) = requested_target_view {
            (target, "prospective", true)
        } else if let Some(target) = draft_target_view {
            (target, "draft", true)
        } else {
            let target = target_catalog
                .compatible_targets
                .iter()
                .find(|target| target.agent_ref.is_none() && target.runtime_profile_ref == "native")
                .or_else(|| {
                    target_catalog
                        .compatible_targets
                        .iter()
                        .find(|target| target.ready)
                })
                .or_else(|| target_catalog.compatible_targets.first())
                .cloned()
                .ok_or_else(|| {
                    runtime_rpc_error(
                        "target_catalog_empty",
                        "configuration",
                        wire::RuntimeRetryClassView::UserAction,
                        "No compatible Agent targets are configured.".to_string(),
                        None,
                    )
                })?;
            (target, "default", false)
        };
    let runtime_ref = selected_target.runtime_profile_ref.clone();
    if !profiles.iter().any(|profile| profile.id == runtime_ref) {
        return Err(runtime_rpc_error(
            "runtime_profile_not_found",
            "configuration",
            wire::RuntimeRetryClassView::UserAction,
            format!("Unknown Runtime Profile `{runtime_ref}`."),
            None,
        ));
    }
    let selection_state = selection_state.to_string();
    let draft_preparation_problem = binding
        .is_none()
        .then(|| {
            source_lane
                .as_ref()
                .and_then(|lane| source_lane_preparation_problem(lane, &selected_target))
        })
        .flatten();
    let capability_revision = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .map(|profile| profile.capability_revision.clone())
        .unwrap_or_default();
    let selected_record = bound_profile_record
        .as_ref()
        .filter(|record| record.config.id == runtime_ref)
        .or_else(|| profile_records.get(&runtime_ref));
    let selected_profile = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .expect("selected Runtime Profile was validated above");
    let selected_ready = selected_target.ready;
    let selected_health_summary = selected_profile.health.summary.clone();
    let mut surface = selected_record
        .map(|record| {
            profile_agent_surface_descriptor(
                &record.config,
                binding.is_some(),
                capability_revision.clone(),
                selected_ready,
                &selected_health_summary,
            )
        })
        .unwrap_or_default();
    if selected_record.is_some_and(|record| record.config.runtime == RuntimeProfileKind::Native) {
        populate_native_control_catalog(&run_options, &configured, &mut surface.controls);
    } else {
        decorate_configured_model_control_labels(&configured, &mut surface.controls);
    }
    apply_control_state_precedence(
        &mut surface.controls,
        binding.as_ref(),
        source_lane.as_ref(),
    );
    let binding_view = binding.as_ref().map(|binding| {
        let binding_runtime_ref = binding.runtime_ref.as_deref().unwrap_or("unresolved");
        let session_handle = binding
            .native_session_id
            .as_deref()
            .map(|native_session_id| {
                if binding_runtime_ref == "native" {
                    binding.thread_id.clone()
                } else {
                    crate::runtime_session_handle(
                        binding_runtime_ref,
                        Path::new(&binding.cwd),
                        native_session_id,
                    )
                }
            });
        wire::RuntimeBindingView {
            thread_id: binding.thread_id.clone(),
            agent_ref: binding.agent_ref.clone(),
            agent_fingerprint: binding.agent_fingerprint.clone().unwrap_or_default(),
            runtime_ref: binding_runtime_ref.to_string(),
            backend_kind: binding
                .backend_kind
                .clone()
                .unwrap_or_else(|| "unresolved".to_string()),
            native_kind: binding.native_kind.clone(),
            native_session_id: session_handle,
            cwd: binding.cwd.clone(),
            profile_fingerprint: binding.profile_fingerprint.clone().unwrap_or_default(),
            ownership: match binding.ownership {
                GatewayRuntimeBindingOwnership::ReadWrite => {
                    wire::RuntimeBindingOwnershipView::ReadWrite
                }
                GatewayRuntimeBindingOwnership::ReadOnly => {
                    wire::RuntimeBindingOwnershipView::ReadOnly
                }
            },
            binding_revision: u64::try_from(binding.binding_revision).unwrap_or_default(),
        }
    });
    let stability = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .and_then(|profile| profile.stability);
    let mut compatible_targets = target_catalog.compatible_targets.clone();
    if !compatible_targets
        .iter()
        .any(|target| target.target_id == selected_target.target_id)
    {
        compatible_targets.push(selected_target.clone());
    }
    let history = surface.history.clone();
    let actions = thread_action_descriptors(
        state,
        scope,
        thread_id.as_deref(),
        &surface.actions,
        selected_ready,
        stability,
    )?;
    let pending_interactions = thread_pending_interactions(state, scope, thread_id.as_deref())?;
    let target_revision = binding
        .as_ref()
        .map(|binding| public_redacted_bound_target_revision(&selected_target, binding))
        .or_else(|| {
            target_catalog
                .target_revision(&selected_target.target_id)
                .map(str::to_string)
        })
        .unwrap_or_else(|| capability_revision.clone());
    let preparation_revision = draft_preparation_problem
        .as_ref()
        .map(|problem| format!("draft-prepare:{}:{}", problem.code, problem.message))
        .unwrap_or_default();
    let context_revision = combined_thread_revision(&[
        &target_revision,
        &selected_profile.health.status,
        &binding
            .as_ref()
            .map(|binding| binding.binding_revision.to_string())
            .unwrap_or_default(),
        &preparation_revision,
    ]);
    let control_revision = binding
        .as_ref()
        .map(|binding| binding.control_revision.to_string())
        .unwrap_or_else(|| {
            source_draft_control_revision(source_lane.as_ref(), &capability_revision)
        });
    let missing_required_control = surface.controls.iter().find(|control| {
        control.required && (!control.enabled || control.effective_value.is_none())
    });
    let any_input_enabled = surface
        .input_capabilities
        .iter()
        .any(|capability| capability.enabled && capability.kind != "agentMention");
    let read_only = binding
        .as_ref()
        .is_some_and(|binding| binding.ownership != GatewayRuntimeBindingOwnership::ReadWrite);
    let unavailable_recovery_action = selected_record
        .filter(|record| {
            record.config.backend_ref.as_deref() == Some(crate::managed_acp::CODEX_ACP_BACKEND_ID)
        })
        .and_then(|_| match selected_profile.health.status.as_str() {
            "missing" => Some("backend/install".to_string()),
            "error" => Some("backend/repair".to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "backend/doctor".to_string());
    let (sendable, sendability_reason, recovery_action) = if !explicit_selection {
        (
            false,
            Some("Select an Agent target before starting a turn.".to_string()),
            None,
        )
    } else if let Some(problem) = draft_preparation_problem {
        (
            false,
            Some(problem.message),
            Some("thread/draft/prepare".to_string()),
        )
    } else if !selected_ready {
        (
            false,
            Some(selected_health_summary.clone()),
            Some(unavailable_recovery_action),
        )
    } else if read_only {
        (
            false,
            Some("This Thread binding is read-only.".to_string()),
            Some("thread/draft/open".to_string()),
        )
    } else if let Some(control) = missing_required_control {
        (
            false,
            Some(control.unavailable_reason.clone().unwrap_or_else(|| {
                format!("{} is required before starting a turn.", control.label)
            })),
            None,
        )
    } else if !any_input_enabled {
        (
            false,
            Some(
                "This Agent target exposes no input kind implemented by ThreadApplication."
                    .to_string(),
            ),
            Some("backend/doctor".to_string()),
        )
    } else {
        (true, None, None)
    };
    let projected_target_id = selected_target.target_id;
    Ok((
        wire::ThreadContextReadResult {
            selected_target_id: explicit_selection.then(|| projected_target_id.clone()),
            suggested_target_id: (!explicit_selection).then_some(projected_target_id),
            runtime_profile_ref: runtime_ref,
            selection_state,
            profiles,
            binding: binding_view,
            controls: surface.controls,
            stability,
            capabilities: surface.capabilities,
            compatible_targets,
            input_capabilities: surface.input_capabilities,
            actions,
            sendability: wire::ThreadSendabilityView {
                allowed: sendable,
                reason: sendability_reason,
                recovery_action,
            },
            history,
            pending_interactions,
            context_revision,
            control_revision,
        },
        configured,
    ))
}

pub(super) async fn thread_context_read_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
) -> psychevo_runtime::Result<wire::ThreadContextReadResult> {
    let target_catalog = RunnableTargetCatalog::load(state, scope)?;
    thread_context_read_result_live_with_catalog(state, scope, params, target_catalog).await
}

pub(super) async fn thread_context_read_result_live_with_catalog(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
    target_catalog: Arc<RunnableTargetCatalog>,
) -> psychevo_runtime::Result<wire::ThreadContextReadResult> {
    thread_context_read_result_live_with_catalog_and_configured(
        state,
        scope,
        params,
        target_catalog,
    )
    .await
    .map(|(context, _)| context)
}

pub(super) async fn thread_context_read_result_live_with_catalog_and_configured(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadContextReadParams,
    target_catalog: Arc<RunnableTargetCatalog>,
) -> psychevo_runtime::Result<(
    wire::ThreadContextReadResult,
    Vec<psychevo_runtime::ConfiguredModel>,
)> {
    let thread_id = match params.thread_id.clone() {
        Some(thread_id) => Some(thread_id),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let (mut context, configured) =
        thread_context_read_result_with_catalog(state, scope, params, target_catalog)?;
    let Some(thread_id) = thread_id else {
        let source_key = scope.source.source_key();
        if let Some(target_id) = context.selected_target_id.as_deref()
            && let Some(snapshot) = state
                .inner
                .gateway
                .inspect_prepared_agent_session(&source_key.0, target_id)
                .await?
        {
            apply_prepared_acp_snapshot(state, scope, &configured, &mut context, &snapshot)?;
        }
        return Ok((context, configured));
    };
    let Some(binding) = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)?
    else {
        return Ok((context, configured));
    };
    if binding.status != GatewayRuntimeBindingStatus::Resolved
        || binding.native_kind.as_deref() != Some(RuntimeProfileKind::Acp.as_str())
    {
        return Ok((context, configured));
    }
    let Some(native_session_id) = binding.native_session_id.clone() else {
        return Ok((context, configured));
    };
    let resident_snapshot = state
        .inner
        .gateway
        .inspect_cached_bound_agent_session(thread_id.clone(), native_session_id)
        .await?;
    let resident = resident_snapshot.is_some();
    let snapshot = match resident_snapshot {
        Some(snapshot) => snapshot,
        None => match persisted_acp_session_snapshot(state, &thread_id)? {
            Some(snapshot) => snapshot,
            None => return Ok((context, configured)),
        },
    };
    let profile_capability_revision = context
        .profiles
        .iter()
        .find(|profile| profile.id == context.runtime_profile_ref)
        .map(|profile| profile.capability_revision.clone())
        .unwrap_or_default();
    let capability_revision =
        combined_thread_revision(&[&profile_capability_revision, &snapshot.control_revision]);
    let mut surface = acp_session_agent_surface_descriptor(&snapshot, capability_revision);
    decorate_configured_model_control_labels(&configured, &mut surface.controls);
    apply_control_state_precedence(&mut surface.controls, Some(&binding), None);
    context.controls = surface.controls;
    context.input_capabilities = surface.input_capabilities;
    context.capabilities = surface.capabilities;
    context.history = surface.history;
    context.actions = thread_action_descriptors(
        state,
        scope,
        Some(&thread_id),
        &surface.actions,
        context.sendability.allowed,
        context.stability,
    )?;
    let supports_product_input = context
        .input_capabilities
        .iter()
        .any(|capability| capability.enabled && capability.kind != "agentMention");
    if !supports_product_input {
        context.sendability.allowed = false;
        context.sendability.reason = Some(
            "The ACP Agent did not negotiate any input kind supported by the public Thread contract."
                .to_string(),
        );
        context.sendability.recovery_action = Some("backend/doctor".to_string());
    }
    context.context_revision =
        combined_thread_revision(&[&context.context_revision, &snapshot.admission_revision()]);
    context.control_revision = combined_thread_revision(&[
        &binding.control_revision.to_string(),
        &snapshot.control_revision,
    ]);
    if !resident && !snapshot.history.resumable {
        let reason = "This process-ephemeral ACP Thread cannot be resumed after process restart. Start a new Thread.";
        context.history.fidelity = wire::ThreadHistoryFidelityView::Partial;
        context.history.hint = Some(reason.to_string());
        context.sendability = wire::ThreadSendabilityView {
            allowed: false,
            reason: Some(reason.to_string()),
            recovery_action: Some("thread/draft/open".to_string()),
        };
        context.context_revision =
            combined_thread_revision(&[&context.context_revision, "process-ephemeral-unavailable"]);
    }
    Ok((context, configured))
}

fn apply_prepared_acp_snapshot(
    state: &WebState,
    scope: &ResolvedScope,
    configured: &[psychevo_runtime::ConfiguredModel],
    context: &mut wire::ThreadContextReadResult,
    snapshot: &crate::acp_peer::AcpSessionSnapshot,
) -> psychevo_runtime::Result<()> {
    let profile_capability_revision = context
        .profiles
        .iter()
        .find(|profile| profile.id == context.runtime_profile_ref)
        .map(|profile| profile.capability_revision.clone())
        .unwrap_or_default();
    let capability_revision =
        combined_thread_revision(&[&profile_capability_revision, &snapshot.control_revision]);
    let mut surface = acp_session_agent_surface_descriptor(snapshot, capability_revision);
    decorate_configured_model_control_labels(configured, &mut surface.controls);
    let source_lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&scope.source.source_key().0)?;
    apply_control_state_precedence(&mut surface.controls, None, source_lane.as_ref());
    context.controls = surface.controls;
    context.input_capabilities = surface.input_capabilities;
    context.capabilities = surface.capabilities;
    context.context_revision =
        combined_thread_revision(&[&context.context_revision, &snapshot.admission_revision()]);
    context.control_revision =
        combined_thread_revision(&[&context.control_revision, &snapshot.control_revision]);
    Ok(())
}

pub(super) fn prepare_draft_source_lane(
    state: &WebState,
    scope: &ResolvedScope,
    target: &wire::RunnableTargetView,
) -> psychevo_runtime::Result<()> {
    let source_key = scope.source.source_key();
    let existing_lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source_key.0)?;
    let same_target = existing_lane.as_ref().is_some_and(|lane| {
        lane.draft_agent_ref == target.agent_ref
            && lane.draft_profile_ref.as_deref() == Some(target.runtime_profile_ref.as_str())
    });
    let draft_control_values = if same_target {
        existing_lane
            .as_ref()
            .map(|lane| lane.draft_control_values.clone())
            .unwrap_or_default()
    } else {
        Default::default()
    };
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &scope.source.kind,
            raw_identity: scope.source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: scope.source.visible_name.as_deref(),
            thread_id: None,
            draft_agent_ref: target.agent_ref.as_deref(),
            draft_profile_ref: Some(&target.runtime_profile_ref),
            draft_control_values: &draft_control_values,
            lineage: Some(json!({"reason": "thread_draft_prepare"})),
        })?;
    state.inner.gateway.bump_source_generation_key(&source_key);
    Ok(())
}

fn ensure_draft_source_unbound(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<()> {
    if state
        .inner
        .gateway
        .resolve_source_thread(&scope.source)?
        .is_some()
    {
        return Err(runtime_rpc_error(
            "immutable_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "Start a new Thread before preparing a different Agent target.".to_string(),
            None,
        ));
    }
    Ok(())
}

pub(super) async fn thread_draft_prepare_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadDraftPrepareParams,
) -> psychevo_runtime::Result<wire::ThreadDraftPrepareResult> {
    let target_catalog = RunnableTargetCatalog::load(state, scope)?;
    thread_draft_prepare_result_with_catalog(state, scope, params, target_catalog).await
}

pub(super) async fn thread_draft_prepare_result_with_catalog(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadDraftPrepareParams,
    target_catalog: Arc<RunnableTargetCatalog>,
) -> psychevo_runtime::Result<wire::ThreadDraftPrepareResult> {
    ensure_draft_source_unbound(state, scope)?;
    let target = target_catalog
        .by_id(&params.target_id)
        .cloned()
        .ok_or_else(|| {
            agent_session_error(
                "target_not_found",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "The selected Agent target is no longer present in this workspace catalog. Refresh Thread Context and select another target.",
                None,
            )
        })?;
    let source_lane_prepared = if target.ready {
        prepare_draft_source_lane(state, scope, &target)?;
        true
    } else {
        false
    };
    let (context, configured) = thread_context_read_result_live_with_catalog_and_configured(
        state,
        scope,
        wire::ThreadContextReadParams {
            thread_id: None,
            target: Some(runnable_target_input(&target)),
            scope: Some(params.scope.clone()),
        },
        target_catalog.clone(),
    )
    .await?;
    thread_draft_prepare_result_with_work(
        state,
        scope,
        params,
        ThreadDraftPrepareWork {
            target_catalog,
            target,
            context,
            configured,
            source_lane_prepared,
        },
    )
    .await
}

pub(super) async fn thread_draft_prepare_result_with_work(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadDraftPrepareParams,
    work: ThreadDraftPrepareWork,
) -> psychevo_runtime::Result<wire::ThreadDraftPrepareResult> {
    ensure_draft_source_unbound(state, scope)?;
    let ThreadDraftPrepareWork {
        target_catalog,
        target,
        mut context,
        configured,
        source_lane_prepared,
    } = work;
    if target.target_id != params.target_id {
        return Err(agent_session_error(
            "target_not_found",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            "The selected Agent target is no longer present in this workspace catalog. Refresh Thread Context and select another target.",
            None,
        ));
    }
    if !target.ready {
        let problem = wire::RuntimeErrorView {
            code: "runtime_unavailable".to_string(),
            stage: "configuration".to_string(),
            retry_class: wire::RuntimeRetryClassView::UserAction,
            message: target
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| "The selected Agent target is not ready.".to_string()),
            diagnostic_ref: None,
        };
        return Ok(wire::ThreadDraftPrepareResult {
            context,
            problem: Some(problem),
        });
    }
    if !source_lane_prepared {
        prepare_draft_source_lane(state, scope, &target)?;
    }
    let source_key = scope.source.source_key();
    let draft_control_values = state
        .inner
        .state
        .store()
        .gateway_source_lane(&source_key.0)?
        .map(|lane| lane.draft_control_values)
        .unwrap_or_default();

    let record = target_catalog
        .profile_records
        .get(&target.runtime_profile_ref)
        .ok_or_else(|| {
            runtime_rpc_error(
                "runtime_profile_not_found",
                "configuration",
                wire::RuntimeRetryClassView::UserAction,
                format!("Unknown Runtime Profile `{}`.", target.runtime_profile_ref),
                None,
            )
        })?;
    if record.config.runtime == RuntimeProfileKind::Acp {
        gateway_profile_mark(
            "thread_draft_acp_handshake_started",
            None,
            None,
            GatewayProfileFields {
                request_method: Some("thread/draft/open"),
                runtime_source: Some("web"),
                ..GatewayProfileFields::default()
            },
        );
        let preparation = async {
            let peer = resolve_runtime_target_peer_turn_with_catalog(
                state,
                scope,
                &target.runtime_profile_ref,
                target.agent_ref.as_deref(),
                &target_catalog,
            )?
            .ok_or_else(|| {
                runtime_rpc_error(
                    "runtime_unavailable",
                    "configuration",
                    wire::RuntimeRetryClassView::UserAction,
                    "The selected ACP Agent target is unavailable.".to_string(),
                    None,
                )
            })?;
            let mut options = state.run_options(scope.cwd.clone(), None);
            options.runtime_ref = Some(target.runtime_profile_ref.clone());
            options.agent = target.agent_ref.clone();
            state
                .inner
                .gateway
                .prepare_agent_session(
                    peer,
                    options,
                    source_key.0,
                    target.target_id.clone(),
                    target.agent_ref.clone(),
                )
                .await
        }
        .await;
        gateway_profile_mark(
            "thread_draft_acp_handshake_completed",
            None,
            None,
            GatewayProfileFields {
                request_method: Some("thread/draft/open"),
                runtime_source: Some("web"),
                ..GatewayProfileFields::default()
            },
        );
        match preparation {
            Ok(snapshot) => {
                apply_prepared_acp_snapshot(state, scope, &configured, &mut context, &snapshot)?
            }
            Err(error) => {
                let problem = runtime_problem_view(&error);
                persist_source_lane_preparation_problem(
                    state,
                    scope,
                    &target,
                    &draft_control_values,
                    &problem,
                )?;
                apply_draft_preparation_problem(&mut context, &problem);
                return Ok(wire::ThreadDraftPrepareResult {
                    context,
                    problem: Some(problem),
                });
            }
        }
    }
    Ok(wire::ThreadDraftPrepareResult {
        context,
        problem: None,
    })
}

fn apply_draft_preparation_problem(
    context: &mut wire::ThreadContextReadResult,
    problem: &wire::RuntimeErrorView,
) {
    let preparation_revision = format!("draft-prepare:{}:{}", problem.code, problem.message);
    context.context_revision =
        combined_thread_revision(&[&context.context_revision, &preparation_revision]);
    context.sendability = wire::ThreadSendabilityView {
        allowed: false,
        reason: Some(problem.message.clone()),
        recovery_action: Some("thread/draft/prepare".to_string()),
    };
}

const DRAFT_PREPARATION_PROBLEM_KEY: &str = "draftPreparationProblem";

fn source_lane_preparation_problem(
    lane: &psychevo_runtime::GatewaySourceLaneRecord,
    target: &wire::RunnableTargetView,
) -> Option<wire::RuntimeErrorView> {
    if lane.draft_agent_ref != target.agent_ref
        || lane.draft_profile_ref.as_deref() != Some(target.runtime_profile_ref.as_str())
    {
        return None;
    }
    serde_json::from_value(
        lane.lineage
            .as_ref()?
            .get(DRAFT_PREPARATION_PROBLEM_KEY)?
            .clone(),
    )
    .ok()
}

fn persist_source_lane_preparation_problem(
    state: &WebState,
    scope: &ResolvedScope,
    target: &wire::RunnableTargetView,
    draft_control_values: &BTreeMap<String, String>,
    problem: &wire::RuntimeErrorView,
) -> psychevo_runtime::Result<()> {
    let source_key = scope.source.source_key();
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_lane(GatewaySourceLaneInput {
            source_key: &source_key.0,
            source_kind: &scope.source.kind,
            raw_identity: scope.source.raw_identity.clone().unwrap_or(Value::Null),
            visible_name: scope.source.visible_name.as_deref(),
            thread_id: None,
            draft_agent_ref: target.agent_ref.as_deref(),
            draft_profile_ref: Some(&target.runtime_profile_ref),
            draft_control_values,
            lineage: Some(json!({
                "reason": "thread_draft_prepare_failed",
                (DRAFT_PREPARATION_PROBLEM_KEY): problem,
            })),
        })?;
    state.inner.gateway.bump_source_generation_key(&source_key);
    Ok(())
}

fn runtime_problem_view(error: &Error) -> wire::RuntimeErrorView {
    let data = error.structured_data();
    let nested = data.and_then(|value| value.get("error"));
    let field = |name: &str| {
        data.and_then(|value| value.get(name))
            .or_else(|| nested.and_then(|value| value.get(name)))
            .and_then(Value::as_str)
    };
    let retry_class = match field("retryClass") {
        Some("never") => wire::RuntimeRetryClassView::Never,
        Some("safeRetry" | "safe_retry" | "retry") => wire::RuntimeRetryClassView::SafeRetry,
        Some("reconnect") => wire::RuntimeRetryClassView::Reconnect,
        Some("unknownDelivery" | "unknown_delivery") => {
            wire::RuntimeRetryClassView::UnknownDelivery
        }
        Some("userAction" | "user_action") | Some(_) | None => {
            wire::RuntimeRetryClassView::UserAction
        }
    };
    wire::RuntimeErrorView {
        code: field("code").unwrap_or("runtime_unavailable").to_string(),
        stage: field("stage").unwrap_or("configuration").to_string(),
        retry_class,
        message: error.to_string(),
        diagnostic_ref: field("diagnosticRef").map(str::to_string),
    }
}

pub(super) fn selected_context_target_id(
    context: &wire::ThreadContextReadResult,
) -> psychevo_runtime::Result<&str> {
    context.selected_target_id.as_deref().ok_or_else(|| {
        runtime_rpc_error(
            "target_not_selected",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "Select an Agent target before performing this operation.".to_string(),
            None,
        )
    })
}

fn persisted_acp_session_snapshot(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<Option<crate::acp_peer::AcpSessionSnapshot>> {
    let projection = state
        .inner
        .state
        .store()
        .session_metadata(thread_id)?
        .and_then(|metadata| {
            metadata
                .get(ACP_PEER_METADATA_KEY)
                .and_then(|peer| peer.get("sessionProjection"))
                .cloned()
        });
    projection
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| {
            agent_session_error(
                "acp_projection_invalid",
                AgentErrorStage::History,
                "never",
                "not_delivered",
                format!("Persisted ACP session projection is invalid: {error}"),
                Some(format!("acp-session:{thread_id}")),
            )
        })
}

pub(super) fn cached_thread_history_descriptor(
    state: &WebState,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ThreadHistoryView> {
    let Some(thread_id) = thread_id else {
        return Ok(unavailable_history(
            "History becomes available after the public Thread is bound.",
        ));
    };
    let Some(binding) = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(thread_id)?
    else {
        return Ok(unavailable_history(
            "History is unavailable until the Agent target is bound.",
        ));
    };
    if binding.status != GatewayRuntimeBindingStatus::Resolved {
        return Ok(unavailable_history(
            "History is unavailable until the Agent target binding is resolved.",
        ));
    }
    if binding.backend_kind.as_deref() == Some("acp")
        && let Some(snapshot) = persisted_acp_session_snapshot(state, thread_id)?
    {
        return Ok(acp_session_agent_surface_descriptor(&snapshot, String::new()).history);
    }
    let profile = bound_runtime_profile_record(&binding)?;
    Ok(profile_agent_surface_descriptor(
        &profile.config,
        true,
        binding.adapter_revision.clone().unwrap_or_default(),
        true,
        "",
    )
    .history)
}

pub(super) async fn thread_control_set_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::ThreadControlSetParams,
) -> psychevo_runtime::Result<wire::ThreadControlSetResult> {
    let thread_id = params.thread_id.clone().or_else(|| {
        state
            .inner
            .gateway
            .resolve_source_thread(&scope.source)
            .ok()
            .flatten()
    });
    let effective_scope = match thread_id.as_deref() {
        Some(thread_id) => resolved_scope_for_thread(state, thread_id)?,
        None => scope.clone(),
    };
    let binding = thread_id
        .as_deref()
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten();
    if let Some(binding) = binding.as_ref()
        && binding.ownership != GatewayRuntimeBindingOwnership::ReadWrite
    {
        return Err(runtime_rpc_error(
            "read_only_session",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "This Runtime session is read-only.".to_string(),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let prospective_target = binding
        .is_none()
        .then(|| runnable_target_by_id(state, &effective_scope, &params.target_id))
        .transpose()?;
    let context = thread_context_read_result_live(
        state,
        &effective_scope,
        wire::ThreadContextReadParams {
            thread_id: thread_id.clone(),
            target: prospective_target.as_ref().map(runnable_target_input),
            scope: params.scope.clone(),
        },
    )
    .await?;
    if context.selected_target_id.as_deref() != Some(params.target_id.as_str()) {
        return Err(runtime_rpc_error(
            "target_changed",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "The selected Agent target changed; refresh Thread Context before changing this control."
                .to_string(),
            thread_id
                .as_ref()
                .map(|thread_id| format!("runtime-binding:{thread_id}")),
        ));
    }
    let runtime_profile_ref = context.runtime_profile_ref.clone();
    if let Some(binding) = binding.as_ref()
        && binding.runtime_ref.as_deref() != Some(runtime_profile_ref.as_str())
    {
        return Err(runtime_rpc_error(
            "immutable_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            format!(
                "Thread `{}` is not bound to Runtime Profile `{}`.",
                binding.thread_id, runtime_profile_ref
            ),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let binding_revision = binding
        .as_ref()
        .and_then(|binding| u64::try_from(binding.binding_revision).ok())
        .unwrap_or_default();
    let before = context
        .controls
        .iter()
        .find(|control| control.id == params.control_id)
        .cloned()
        .ok_or_else(|| {
            runtime_rpc_error(
                "control_not_found",
                "control",
                wire::RuntimeRetryClassView::UserAction,
                format!(
                    "This Thread does not expose control `{}`.",
                    params.control_id
                ),
                thread_id
                    .as_ref()
                    .map(|thread_id| format!("runtime-binding:{thread_id}")),
            )
        })?;
    if !before.enabled {
        return Err(runtime_rpc_error(
            "control_unavailable",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            before
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| format!("Control `{}` is unavailable.", before.id)),
            thread_id
                .as_ref()
                .map(|thread_id| format!("runtime-binding:{thread_id}")),
        ));
    }
    if params.expected_binding_revision != binding_revision
        || params.expected_context_revision != context.context_revision
        || params.expected_control_revision != context.control_revision
        || params.expected_capability_revision != before.capability_revision
    {
        return Err(runtime_rpc_error(
            "stale_revision",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            "Thread Context changed; refresh it before changing this control.".to_string(),
            thread_id
                .as_ref()
                .map(|thread_id| format!("runtime-binding:{thread_id}")),
        ));
    }
    validate_control_value(&before, &params.value)?;
    let changed = before.effective_value.as_ref() != Some(&params.value);

    let Some(binding) = binding else {
        let source_key = effective_scope.source.source_key();
        let prepared_snapshot = state
            .inner
            .gateway
            .set_prepared_agent_session_control(
                &source_key.0,
                &params.target_id,
                params.control_id.clone(),
                params.value.clone(),
            )
            .await?;
        let lane = state
            .inner
            .state
            .store()
            .gateway_source_lane(&source_key.0)?;
        let mut draft_control_values = lane
            .as_ref()
            .map(|lane| lane.draft_control_values.clone())
            .unwrap_or_default();
        draft_control_values.insert(
            params.control_id.clone(),
            thread_control_override_string_value(&params.value)?,
        );
        state
            .inner
            .state
            .store()
            .upsert_gateway_source_lane(GatewaySourceLaneInput {
                source_key: &source_key.0,
                source_kind: &effective_scope.source.kind,
                raw_identity: effective_scope
                    .source
                    .raw_identity
                    .clone()
                    .unwrap_or(Value::Null),
                visible_name: effective_scope.source.visible_name.as_deref(),
                thread_id: None,
                draft_agent_ref: prospective_target
                    .as_ref()
                    .and_then(|target| target.agent_ref.as_deref()),
                draft_profile_ref: Some(&runtime_profile_ref),
                draft_control_values: &draft_control_values,
                lineage: Some(json!({"reason": "thread_application_control"})),
            })?;
        state.inner.gateway.bump_source_generation_key(&source_key);
        let (mut after_context, configured) = thread_context_read_result_with_configured_models(
            state,
            &effective_scope,
            wire::ThreadContextReadParams {
                thread_id: None,
                target: prospective_target.as_ref().map(runnable_target_input),
                scope: params.scope,
            },
        )?;
        if let Some(snapshot) = prepared_snapshot.as_ref() {
            apply_prepared_acp_snapshot(
                state,
                &effective_scope,
                &configured,
                &mut after_context,
                snapshot,
            )?;
        }
        let after = after_context
            .controls
            .iter()
            .find(|control| control.id == params.control_id)
            .cloned()
            .expect("stored source draft control remains described");
        return Ok(wire::ThreadControlSetResult {
            changed,
            status: if prepared_snapshot.is_some()
                && after.effective_value.as_ref() == Some(&params.value)
            {
                wire::ThreadControlReceiptStatusView::Observed
            } else {
                wire::ThreadControlReceiptStatusView::Applied
            },
            control: after,
            binding_revision: 0,
            context_revision: after_context.context_revision.clone(),
            control_revision: after_context.control_revision.clone(),
            context: after_context,
        });
    };

    let mut preferences = binding.thread_preferences.clone();
    preferences.insert(params.control_id.clone(), params.value.clone());
    let mut observed = binding.runtime_observed.clone();
    let active_turn = state
        .activity(&effective_scope.source, Some(&binding.thread_id))
        .running;
    let status = if binding.native_kind.as_deref() == Some(RuntimeProfileKind::Acp.as_str())
        && active_turn
    {
        wire::ThreadControlReceiptStatusView::Stored
    } else if binding.native_kind.as_deref() == Some(RuntimeProfileKind::Acp.as_str()) {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            runtime_rpc_error(
                "runtime_native_session_missing",
                "binding",
                wire::RuntimeRetryClassView::UserAction,
                "ACP session controls require a persisted native session.".to_string(),
                Some(format!("runtime-binding:{}", binding.thread_id)),
            )
        })?;
        let peer = resolve_bound_thread_agent_target(state, &binding)?
            .peer
            .ok_or_else(|| {
                runtime_rpc_error(
                    "runtime_unavailable",
                    "configuration",
                    wire::RuntimeRetryClassView::UserAction,
                    format!(
                        "Runtime Profile `{}` does not resolve to an available ACP backend.",
                        runtime_profile_ref
                    ),
                    None,
                )
            })?;
        let snapshot = state
            .inner
            .gateway
            .set_bound_agent_session_control(
                peer,
                state.run_options(PathBuf::from(&binding.cwd), Some(binding.thread_id.clone())),
                binding.thread_id.clone(),
                native_session_id,
                params.control_id.clone(),
                params.value.clone(),
            )
            .await?;
        let mut observed_controls = acp_runtime_control_descriptors(
            snapshot.options.clone(),
            before.capability_revision.clone(),
        );
        if !observed_controls.iter().any(|control| control.id == "mode")
            && let Some(mode) = acp_session_mode_control_descriptor(
                &snapshot.available_modes,
                snapshot.current_mode_id.as_deref(),
                before.capability_revision.clone(),
            )
        {
            observed_controls.push(mode);
        }
        let observed_control = observed_controls
            .into_iter()
            .find(|control| control.id == params.control_id)
            .ok_or_else(|| {
                runtime_rpc_error(
                    "control_ack_invalid",
                    "control",
                    wire::RuntimeRetryClassView::Never,
                    "ACP control acknowledgement omitted the changed control.".to_string(),
                    Some(format!("runtime-binding:{}", binding.thread_id)),
                )
            })?;
        let runtime_observed = observed_control.effective_value == Some(params.value.clone());
        if runtime_observed {
            observed.insert(params.control_id.clone(), params.value.clone());
            wire::ThreadControlReceiptStatusView::Observed
        } else {
            wire::ThreadControlReceiptStatusView::Applied
        }
    } else {
        wire::ThreadControlReceiptStatusView::Applied
    };
    state
        .inner
        .state
        .store()
        .compare_and_set_gateway_runtime_control_state(
            &binding.thread_id,
            binding.binding_revision,
            binding.control_revision,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&preferences),
                runtime_observed: (binding.native_kind.as_deref()
                    == Some(RuntimeProfileKind::Acp.as_str()))
                .then_some(&observed),
            },
        )
        .map_err(|error| {
            agent_session_error(
                "stale_revision",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                format!("Thread control state changed; refresh and retry: {error}"),
                Some(format!("runtime-binding:{}", binding.thread_id)),
            )
        })?;
    let after_context = thread_context_read_result_live(
        state,
        &effective_scope,
        wire::ThreadContextReadParams {
            thread_id: Some(binding.thread_id.clone()),
            target: None,
            scope: params.scope,
        },
    )
    .await?;
    let after = after_context
        .controls
        .iter()
        .find(|control| control.id == params.control_id)
        .cloned()
        .ok_or_else(|| {
            runtime_rpc_error(
                "control_ack_invalid",
                "control",
                wire::RuntimeRetryClassView::Never,
                "The stored control disappeared from Thread Context.".to_string(),
                Some(format!("runtime-binding:{}", binding.thread_id)),
            )
        })?;
    Ok(wire::ThreadControlSetResult {
        changed,
        status,
        control: after,
        binding_revision,
        context_revision: after_context.context_revision.clone(),
        control_revision: after_context.control_revision.clone(),
        context: after_context,
    })
}

fn validate_control_value(
    control: &wire::ThreadControlDescriptorView,
    value: &Value,
) -> psychevo_runtime::Result<()> {
    let _ = thread_control_override_string_value(value)?;
    if !control.choices.is_empty() && !control.choices.iter().any(|choice| choice.value == *value) {
        return Err(runtime_rpc_error(
            "invalid_control",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            format!(
                "Control `{}` does not accept the requested value.",
                control.id
            ),
            None,
        ));
    }
    if value.as_str().is_some_and(|value| value.trim().is_empty()) {
        return Err(runtime_rpc_error(
            "invalid_control",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            format!("Control `{}` requires a non-empty value.", control.id),
            None,
        ));
    }
    Ok(())
}

pub(super) fn resolve_runtime_ref_peer_turn(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<crate::ResolvedPeerTurn>> {
    resolve_runtime_target_peer_turn(state, scope, runtime_ref, None)
}

pub(super) fn resolve_runtime_target_peer_turn(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    agent_ref: Option<&str>,
) -> psychevo_runtime::Result<Option<crate::ResolvedPeerTurn>> {
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    resolve_runtime_target_peer_turn_with_catalog(state, scope, runtime_ref, agent_ref, &catalog)
}

fn resolve_runtime_target_peer_turn_with_catalog(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    agent_ref: Option<&str>,
    catalog: &RunnableTargetCatalog,
) -> psychevo_runtime::Result<Option<crate::ResolvedPeerTurn>> {
    let runtime_ref = runtime_ref.trim();
    if runtime_ref.is_empty() || runtime_ref == "native" {
        return Ok(None);
    }
    let Some(record) = catalog.profile_records.get(runtime_ref) else {
        return Ok(None);
    };
    if record.config.runtime != RuntimeProfileKind::Acp {
        return Ok(None);
    }
    ensure_managed_codex_profile_ready(state, &record.config)?;
    let backend_ref = record.config.backend_ref.as_deref().ok_or_else(|| {
        Error::Message(format!(
            "ACP runtime profile `{runtime_ref}` is missing backendRef"
        ))
    })?;
    let mut options = state.run_options(scope.cwd.clone(), None);
    options.runtime_ref = Some(backend_ref.to_string());
    options.agent = agent_ref.map(str::to_string);
    let mut peer = crate::resolve_peer_turn(&options)?;
    if let Some(peer) = peer.as_mut() {
        peer.process_scope_fingerprint = Some(runtime_profile_fingerprint(&record.config));
    }
    Ok(peer)
}

pub(super) fn ensure_turn_runtime_profile_supported(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: Option<&str>,
) -> psychevo_runtime::Result<()> {
    let runtime_ref = runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    if runtime_ref == "native" {
        return Ok(());
    }
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    let Some(record) = catalog.profile_records.get(runtime_ref) else {
        return Err(Error::Message(format!(
            "unknown runtime profile: {runtime_ref}"
        )));
    };
    if !record.config.enabled {
        return Err(Error::Message(format!(
            "runtime profile `{runtime_ref}` is disabled"
        )));
    }
    ensure_managed_codex_profile_ready(state, &record.config)?;
    match record.config.runtime {
        RuntimeProfileKind::Native => Ok(()),
        RuntimeProfileKind::Acp => resolve_runtime_ref_peer_turn(state, scope, runtime_ref)?
            .map(|_| ())
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime profile `{runtime_ref}` references an unavailable ACP backend"
                ))
            }),
    }
}

pub(super) fn validate_turn_runnable_target(
    state: &WebState,
    scope: &ResolvedScope,
    target: &wire::RunnableTargetInput,
) -> psychevo_runtime::Result<ValidatedRunnableTarget> {
    RunnableTargetCatalog::load(state, scope)?.validate(target)
}

pub(super) fn runtime_backend_kind(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<wire::BackendKind> {
    let catalog = RunnableTargetCatalog::load(state, scope)?;
    let record = catalog.profile_records.get(runtime_ref).ok_or_else(|| {
        agent_session_error(
            "runtime_profile_not_found",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            format!("Unknown Runtime Profile `{runtime_ref}`."),
            None,
        )
    })?;
    Ok(match record.config.runtime {
        RuntimeProfileKind::Native => wire::BackendKind::Native,
        RuntimeProfileKind::Acp => wire::BackendKind::Acp,
    })
}

pub(super) fn resolve_bound_thread_agent_target(
    state: &WebState,
    binding: &GatewayRuntimeBindingRecord,
) -> psychevo_runtime::Result<crate::BoundGatewayAgentTarget> {
    let mut options =
        state.run_options(PathBuf::from(&binding.cwd), Some(binding.thread_id.clone()));
    options.runtime_ref = binding.runtime_ref.clone();
    options.agent = binding.agent_ref.clone();
    crate::resolve_bound_gateway_agent_target(&options, binding.runtime_ref.as_deref())?.ok_or_else(
        || {
            agent_session_error(
                "bound_target_missing",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The Thread binding has no captured Agent target.",
                Some(format!("agent-binding:{}", binding.thread_id)),
            )
        },
    )
}

fn compatible_runnable_targets(
    profile_records: &BTreeMap<String, RuntimeProfileRecord>,
    profile_views: &[wire::RuntimeProfileView],
    agents: &AgentCatalog,
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> (Vec<wire::RunnableTargetView>, BTreeMap<String, String>) {
    let profile_views = profile_views
        .iter()
        .map(|profile| (profile.id.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut targets = Vec::new();
    let mut target_revisions = BTreeMap::new();
    for record in profile_records.values() {
        let Some(profile) = profile_views.get(record.config.id.as_str()).copied() else {
            continue;
        };
        let ready =
            profile.enabled && matches!(profile.health.status.as_str(), "ready" | "unchecked");
        let unavailable_reason = (!ready).then(|| profile.health.summary.clone());
        if record.config.runtime == RuntimeProfileKind::Native {
            let target =
                runnable_target_view(None, "Psychevo", profile, ready, unavailable_reason.clone());
            target_revisions.insert(
                target.target_id.clone(),
                runnable_target_context_revision(&target, &record.config, None, None, profile),
            );
            targets.push(target);
        }
        for agent in &agents.agents {
            if crate::agent_definition_matches_runtime_profile(agent, &record.config) {
                let backend = record
                    .config
                    .backend_ref
                    .as_deref()
                    .and_then(|backend_ref| backends.get(backend_ref));
                let target = runnable_target_view(
                    Some(agent.name.clone()),
                    &agent.name,
                    profile,
                    ready,
                    unavailable_reason.clone(),
                );
                target_revisions.insert(
                    target.target_id.clone(),
                    runnable_target_context_revision(
                        &target,
                        &record.config,
                        Some(agent),
                        backend,
                        profile,
                    ),
                );
                targets.push(target);
            }
        }
    }
    targets.sort_by_key(|target| {
        (
            target.agent_ref.is_some(),
            target.runtime_profile_ref.clone(),
            target.agent_ref.clone().unwrap_or_default(),
        )
    });
    (targets, target_revisions)
}

fn runnable_target_view(
    agent_ref: Option<String>,
    agent_label: &str,
    profile: &wire::RuntimeProfileView,
    ready: bool,
    unavailable_reason: Option<String>,
) -> wire::RunnableTargetView {
    let target_id = runnable_target_id(agent_ref.as_deref(), &profile.id);
    wire::RunnableTargetView {
        target_id,
        agent_ref,
        runtime_profile_ref: profile.id.clone(),
        agent_label: agent_label.to_string(),
        profile_label: profile.label.clone(),
        label: format!("{agent_label} · {}", profile.label),
        ready,
        unavailable_reason,
    }
}

fn runnable_target_id(agent_ref: Option<&str>, runtime_profile_ref: &str) -> String {
    let canonical = serde_json::to_string(&(agent_ref, runtime_profile_ref))
        .expect("RunnableTarget identity serializes");
    format!("target:{}", stable_hash_hex(&canonical))
}

fn runnable_target_context_revision(
    target: &wire::RunnableTargetView,
    profile: &RuntimeProfileConfig,
    agent: Option<&AgentDefinition>,
    backend: Option<&AgentBackendConfig>,
    profile_view: &wire::RuntimeProfileView,
) -> String {
    let agent_fingerprint = agent
        .map(|agent| {
            serde_json::to_value(agent)
                .map(|value| stable_hash_hex(&public_redacted_agent_structure(&value).to_string()))
                .unwrap_or_default()
        })
        .unwrap_or_else(|| {
            stable_hash_hex(&json!({"kind": "psychevo.default-agent", "version": 1}).to_string())
        });
    let backend_structure = backend
        .map(redacted_backend_structure)
        .unwrap_or(Value::Null);
    let backend_revision = stable_hash_hex(&backend_structure.to_string());
    let effective_mcp_names = agent
        .map(|agent| {
            let agent_names = &agent.tool_policy.mcp_servers;
            match backend {
                Some(backend) if !backend.mcp_servers.is_empty() => agent_names
                    .intersection(&backend.mcp_servers)
                    .cloned()
                    .collect::<Vec<_>>(),
                _ => agent_names.iter().cloned().collect::<Vec<_>>(),
            }
        })
        .unwrap_or_default();
    let mcp_revision =
        stable_hash_hex(&serde_json::to_string(&effective_mcp_names).expect("MCP names serialize"));
    combined_thread_revision(&[
        &target.target_id,
        &agent_fingerprint,
        &stable_hash_hex(&public_redacted_profile_structure(profile).to_string()),
        &backend_revision,
        &mcp_revision,
        &profile_view.health.status,
    ])
}

fn public_redacted_agent_structure(value: &Value) -> Value {
    let object = value.as_object();
    let tool_policy = object
        .and_then(|object| object.get("tool_policy"))
        .and_then(Value::as_object);
    let backend_ref = object
        .and_then(|object| object.get("backend"))
        .and_then(Value::as_object)
        .and_then(|backend| backend.get("ref").or_else(|| backend.get("name")))
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "name": object.and_then(|object| object.get("name")).cloned().unwrap_or(Value::Null),
        "enabled": object.and_then(|object| object.get("enabled")).cloned().unwrap_or(Value::Null),
        "source": object.and_then(|object| object.get("source")).cloned().unwrap_or(Value::Null),
        "backendRef": backend_ref,
        "entrypoints": object.and_then(|object| object.get("entrypoints")).cloned().unwrap_or_else(|| json!([])),
        "skills": object.and_then(|object| object.get("skills")).cloned().unwrap_or_else(|| json!([])),
        "optionalContributions": object.and_then(|object| object.get("optional_contributions")).cloned().unwrap_or_else(|| json!([])),
        "toolPolicy": {
            "allowed": tool_policy.and_then(|policy| policy.get("allowed")).cloned().unwrap_or(Value::Null),
            "denied": tool_policy.and_then(|policy| policy.get("denied")).cloned().unwrap_or_else(|| json!([])),
            "allowedAgents": tool_policy.and_then(|policy| policy.get("allowed_agents")).cloned().unwrap_or(Value::Null),
            "deniedAgents": tool_policy.and_then(|policy| policy.get("denied_agents")).cloned().unwrap_or_else(|| json!([])),
            "permissionMode": tool_policy.and_then(|policy| policy.get("permission_mode")).cloned().unwrap_or(Value::Null),
            "mcpServers": tool_policy.and_then(|policy| policy.get("mcp_servers")).cloned().unwrap_or_else(|| json!([])),
        },
        "model": object.and_then(|object| object.get("model")).cloned().unwrap_or(Value::Null),
        "hooksConfigured": object.and_then(|object| object.get("hooks")).is_some_and(|value| !value.is_null()),
        "projectInstructions": object.and_then(|object| object.get("project_instructions")).cloned().unwrap_or(Value::Null),
        "background": object.and_then(|object| object.get("background")).cloned().unwrap_or(Value::Null),
        "maxTurns": object.and_then(|object| object.get("max_turns")).cloned().unwrap_or(Value::Null),
        "maxSpawnDepth": object.and_then(|object| object.get("max_spawn_depth")).cloned().unwrap_or(Value::Null),
        "effort": object.and_then(|object| object.get("effort")).cloned().unwrap_or(Value::Null),
        "initialPromptConfigured": object
            .and_then(|object| object.get("initial_prompt"))
            .is_some_and(|value| !value.is_null()),
    })
}

fn public_redacted_profile_structure(profile: &RuntimeProfileConfig) -> Value {
    json!({
        "id": profile.id,
        "runtime": profile.runtime.as_str(),
        "enabled": profile.enabled,
        "backendRef": profile.backend_ref,
        // These values are already explicit product controls. Arbitrary
        // `options` values are intentionally excluded; only their keys are
        // part of the public revision.
        "defaultModel": profile.default_model,
        "defaultMode": profile.default_mode,
        "defaultAgent": profile.default_agent,
        "approvalMode": profile.approval_mode,
        "sandbox": profile.sandbox,
        "workspaceRootCount": profile.workspace_roots.len(),
        "optionKeys": runtime_profile_option_keys(&profile.options),
    })
}

fn public_redacted_bound_target_revision(
    selected_target: &wire::RunnableTargetView,
    binding: &GatewayRuntimeBindingRecord,
) -> String {
    let agent_structure = binding
        .agent_definition_json
        .as_deref()
        .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok())
        .map(|value| public_redacted_agent_structure(&value))
        .unwrap_or_else(|| {
            json!({
                "agentRef": binding.agent_ref,
                "snapshot": "missing-or-invalid",
            })
        });
    let profile_structure = binding
        .profile_config_json
        .as_deref()
        .and_then(|encoded| serde_json::from_str::<RuntimeProfileConfig>(encoded).ok())
        .map(|profile| public_redacted_profile_structure(&profile))
        .unwrap_or_else(|| {
            json!({
                "runtimeRef": binding.runtime_ref,
                "snapshot": "missing-or-invalid",
            })
        });
    combined_thread_revision(&[
        &selected_target.target_id,
        &stable_hash_hex(&agent_structure.to_string()),
        &stable_hash_hex(&profile_structure.to_string()),
        binding.adapter_kind.as_deref().unwrap_or_default(),
        binding.adapter_revision.as_deref().unwrap_or_default(),
    ])
}

/// Public revisions hash only structural backend facts. Command arguments,
/// environment values, cwd values, and resolved paths may contain secrets and
/// belong only to the private ACP process key.
fn redacted_backend_structure(backend: &AgentBackendConfig) -> Value {
    json!({
        "id": backend.id,
        "kind": backend.kind.as_str(),
        "enabled": backend.enabled,
        "commandConfigured": backend.command.is_some(),
        "argCount": backend.args.len(),
        "envKeys": backend.env.keys().collect::<Vec<_>>(),
        "cwdConfigured": !backend.cwd.trim().is_empty(),
        "entrypoints": backend.entrypoints.iter().map(|entrypoint| entrypoint.as_str()).collect::<Vec<_>>(),
        "clientCapabilities": backend.client_capabilities,
        "mcpServers": backend.mcp_servers,
    })
}

pub(super) fn write_runtime_profile(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!(
            "invalid runtime profile id: {}",
            params.id
        )));
    }
    validate_runtime_profile_kind(&params.runtime)?;
    validate_native_runtime_profile_identity(&params.id, &params.runtime)?;
    ensure_profile_config_for_runtime_profile_write(state, scope, params.target)?;
    let value = runtime_profile_config_json(&params)?;
    let target = params.target;
    let config_dir = runtime_profile_config_dir(state, scope, target);
    let result = set_config_value(
        config_dir,
        &format!("runtime_profiles.{}", params.id),
        value,
    )?;
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles.get(&params.id).ok_or_else(|| {
        Error::Message(format!(
            "runtime profile write did not reload: {}",
            params.id
        ))
    })?;
    Ok(serde_json::to_value(wire::RuntimeProfileWriteResult {
        written: true,
        changed: result.changed,
        path: result.path.display().to_string(),
        target,
        profile: runtime_profile_view(state, scope, record, None)?,
    })?)
}

pub(super) fn set_runtime_profile_enabled(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileSetEnabledParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let existing = profiles
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("unknown runtime profile: {}", params.id)))?;
    let write = wire::RuntimeProfileWriteParams {
        id: existing.config.id.clone(),
        target: params.target,
        runtime: existing.config.runtime.as_str().to_string(),
        enabled: Some(params.enabled),
        label: Some(existing.config.label.clone()),
        backend_ref: existing.config.backend_ref.clone(),
        default_model: existing.config.default_model.clone(),
        default_mode: existing.config.default_mode.clone(),
        default_agent: existing.config.default_agent.clone(),
        approval_mode: existing.config.approval_mode.clone(),
        sandbox: existing.config.sandbox.clone(),
        workspace_roots: existing.config.workspace_roots.clone(),
        options: Some(existing.config.options.clone()),
        scope: Some(scope.to_wire_scope()),
    };
    write_runtime_profile(state, scope, write)
}

pub(super) fn delete_runtime_profile(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!(
            "invalid runtime profile id: {}",
            params.id
        )));
    }
    let target = params.target;
    let config_dir = runtime_profile_config_dir(state, scope, target);
    let result = remove_config_value(config_dir, &format!("runtime_profiles.{}", params.id))?;
    Ok(serde_json::to_value(wire::RuntimeProfileDeleteResult {
        deleted: result.changed,
        changed: result.changed,
        id: params.id,
        path: result.path.display().to_string(),
        target,
    })?)
}

fn runtime_profile_records(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<BTreeMap<String, RuntimeProfileRecord>> {
    let configured =
        load_runtime_profile_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    for config in configured.values() {
        validate_native_runtime_profile_identity(&config.id, config.runtime.as_str())?;
    }
    let generated = generated_runtime_profiles_for_backends(&backends);
    let referenced_backends = configured
        .values()
        .chain(generated.iter())
        .filter_map(|config| config.backend_ref.clone())
        .collect::<BTreeSet<_>>();
    let mut records = generated
        .into_iter()
        .map(|config| {
            (
                config.id.clone(),
                RuntimeProfileRecord {
                    config,
                    generated: true,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for config in configured.into_values() {
        records.insert(
            config.id.clone(),
            RuntimeProfileRecord {
                config,
                generated: false,
            },
        );
    }
    for backend in backends.values() {
        if !backend.enabled || referenced_backends.contains(&backend.id) {
            continue;
        }
        let id = format!("acp:{}", backend.id);
        records
            .entry(id.clone())
            .or_insert_with(|| RuntimeProfileRecord {
                config: RuntimeProfileConfig {
                    id,
                    runtime: RuntimeProfileKind::Acp,
                    enabled: true,
                    label: format!("{} (ACP)", backend.label.trim_end_matches("(ACP)").trim()),
                    backend_ref: Some(backend.id.clone()),
                    default_model: None,
                    default_mode: None,
                    default_agent: None,
                    approval_mode: None,
                    sandbox: None,
                    workspace_roots: Vec::new(),
                    options: Value::Null,
                },
                generated: true,
            });
    }
    Ok(records)
}

fn validate_native_runtime_profile_identity(
    id: &str,
    runtime: &str,
) -> psychevo_runtime::Result<()> {
    if (runtime == "native") != (id == "native") {
        return Err(Error::Message(
            "Native execution is the singleton Runtime Profile id `native`".to_string(),
        ));
    }
    Ok(())
}

/// Validate the Team member execution identity at the configuration seam and
/// capture the effective Profile revision that execution must later require.
/// This read is deliberately cache-only: saving or activating a Team never
/// starts a third-party runtime.
pub(super) fn validate_and_capture_team_runtime_members(
    state: &WebState,
    scope: &ResolvedScope,
    agents: &AgentCatalog,
    members: &[AgentTeamMember],
) -> psychevo_runtime::Result<Vec<AgentTeamMember>> {
    let profiles = runtime_profile_records(state, scope)?;
    members
        .iter()
        .map(|member| {
            let mut member = member.clone();
            let agent = agents
                .agents
                .iter()
                .find(|agent| agent.name == member.agent)
                .ok_or_else(|| {
                    Error::Message(format!(
                        "team member `{}` references unavailable Agent Definition `{}`",
                        member.id, member.agent
                    ))
                })?;
            if !agent.supports_entrypoint(AgentEntrypoint::Subagent) {
                return Err(Error::Message(format!(
                    "team member `{}` Agent Definition `{}` does not support subagent execution",
                    member.id, member.agent
                )));
            }
            let runtime_ref = resolve_team_member_runtime_ref(&profiles, agent, &member)?;
            let Some(runtime_ref) = runtime_ref else {
                if !member.runtime_options.is_empty() {
                    return Err(Error::Message(format!(
                        "team member `{}` runtimeOptions require a Runtime Profile",
                        member.id
                    )));
                }
                member.runtime_profile_revision = None;
                return Ok(member);
            };
            let profile = profiles.get(&runtime_ref).ok_or_else(|| {
                Error::Message(format!(
                    "team member `{}` references unknown Runtime Profile `{runtime_ref}`",
                    member.id
                ))
            })?;
            if !profile.config.enabled {
                return Err(Error::Message(format!(
                    "team member `{}` references disabled Runtime Profile `{runtime_ref}`",
                    member.id
                )));
            }
            validate_team_agent_profile_pairing(agent, &profile.config, &member)?;
            validate_team_runtime_options(
                state,
                scope,
                &profile.config,
                &member.runtime_options,
                &member.id,
            )?;
            let fingerprint = runtime_profile_fingerprint(&profile.config);
            let revision = crate::runtime_profile_config_revision(&fingerprint);
            if member
                .runtime_profile_revision
                .is_some_and(|captured| captured != revision)
            {
                return Err(Error::Message(format!(
                    "team member `{}` captured Runtime Profile `{runtime_ref}` revision {}, but the current revision is {revision}; re-save the Team",
                    member.id,
                    member.runtime_profile_revision.unwrap_or_default(),
                )));
            }
            member.runtime_ref = Some(runtime_ref);
            member.runtime_profile_revision = Some(revision);
            Ok(member)
        })
        .collect()
}

fn resolve_team_member_runtime_ref(
    profiles: &BTreeMap<String, RuntimeProfileRecord>,
    agent: &AgentDefinition,
    member: &AgentTeamMember,
) -> psychevo_runtime::Result<Option<String>> {
    if let Some(runtime_ref) = member
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(runtime_ref.to_string()));
    }
    let Some(backend) = agent.backend.as_ref() else {
        return Ok(None);
    };
    let matches = profiles
        .values()
        .filter(|profile| {
            profile.config.runtime == RuntimeProfileKind::Acp
                && profile.config.backend_ref.as_deref() == Some(backend.name.as_str())
        })
        .map(|profile| profile.config.id.clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [runtime_ref] => Ok(Some(runtime_ref.clone())),
        [] => Err(Error::Message(format!(
            "team member `{}` Agent Definition `{}` uses ACP backend `{}`, but no Runtime Profile resolves to that backend",
            member.id, agent.name, backend.name
        ))),
        _ => Err(Error::Message(format!(
            "team member `{}` Agent Definition `{}` matches multiple ACP Runtime Profiles ({}); select runtimeRef explicitly",
            member.id,
            agent.name,
            matches.join(", ")
        ))),
    }
}

fn validate_team_agent_profile_pairing(
    agent: &AgentDefinition,
    profile: &RuntimeProfileConfig,
    member: &AgentTeamMember,
) -> psychevo_runtime::Result<()> {
    match profile.runtime {
        RuntimeProfileKind::Native => {
            if agent.backend.is_some() {
                return Err(Error::Message(format!(
                    "team member `{}` cannot pair backend Agent Definition `{}` with Native Runtime Profile `{}`",
                    member.id, agent.name, profile.id
                )));
            }
        }
        RuntimeProfileKind::Acp => {
            let expected_backend = profile.backend_ref.as_deref().ok_or_else(|| {
                Error::Message(format!(
                    "ACP Runtime Profile `{}` is missing backendRef",
                    profile.id
                ))
            })?;
            let agent_backend = agent.backend.as_ref().map(|backend| backend.name.as_str());
            if agent_backend != Some(expected_backend) {
                return Err(Error::Message(format!(
                    "team member `{}` Agent Definition `{}` uses ACP backend `{}`, but Runtime Profile `{}` resolves to `{expected_backend}`",
                    member.id,
                    agent.name,
                    agent_backend.unwrap_or("none"),
                    profile.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_team_runtime_options(
    _state: &WebState,
    _scope: &ResolvedScope,
    profile: &RuntimeProfileConfig,
    options: &BTreeMap<String, String>,
    member_id: &str,
) -> psychevo_runtime::Result<()> {
    for (key, value) in options {
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(Error::Message(format!(
                "team member `{member_id}` runtime option `{key}` must have a non-empty value"
            )));
        }
        if matches!(
            key.as_str(),
            "approvalMode"
                | "approval_mode"
                | "permissionMode"
                | "permission_mode"
                | "sandbox"
                | "workspaceRoots"
                | "workspace_roots"
        ) {
            return Err(Error::Message(format!(
                "team member `{member_id}` safety override `{key}` must be configured on Runtime Profile `{}`",
                profile.id
            )));
        }
    }
    match profile.runtime {
        RuntimeProfileKind::Native if !options.is_empty() => Err(Error::Message(format!(
            "team member `{member_id}` Native Runtime Profile options are not supported by the managed-child path"
        ))),
        RuntimeProfileKind::Native => Ok(()),
        RuntimeProfileKind::Acp => {
            if let Some(unsupported) = options
                .keys()
                .find(|key| !matches!(key.as_str(), "model" | "mode" | "agent"))
            {
                return Err(Error::Message(format!(
                    "team member `{member_id}` ACP runtime option `{unsupported}` is unsupported by the stable Team contract"
                )));
            }
            Ok(())
        }
    }
}

pub(super) fn generated_runtime_profiles() -> Vec<RuntimeProfileConfig> {
    vec![
        RuntimeProfileConfig {
            id: "native".to_string(),
            runtime: RuntimeProfileKind::Native,
            enabled: true,
            label: "Psychevo (Native)".to_string(),
            backend_ref: None,
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
        RuntimeProfileConfig {
            id: "codex".to_string(),
            runtime: RuntimeProfileKind::Acp,
            enabled: true,
            label: "Codex (ACP)".to_string(),
            backend_ref: Some(crate::managed_acp::CODEX_ACP_BACKEND_ID.to_string()),
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
        RuntimeProfileConfig {
            id: "opencode".to_string(),
            runtime: RuntimeProfileKind::Acp,
            enabled: true,
            label: "OpenCode (ACP)".to_string(),
            backend_ref: Some("opencode".to_string()),
            default_model: None,
            default_mode: Some("build".to_string()),
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
    ]
}

fn generated_runtime_profiles_for_backends(
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> Vec<RuntimeProfileConfig> {
    generated_runtime_profiles()
        .into_iter()
        .filter(|profile| {
            profile
                .backend_ref
                .as_deref()
                .is_none_or(|backend_ref| backends.contains_key(backend_ref))
        })
        .collect()
}

fn runtime_profile_view(
    state: &WebState,
    scope: &ResolvedScope,
    record: &RuntimeProfileRecord,
    checked_at_ms: Option<i64>,
) -> psychevo_runtime::Result<wire::RuntimeProfileView> {
    let backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    runtime_profile_view_with_backends(state, scope, record, checked_at_ms, &backends)
}

fn runtime_profile_view_with_backends(
    state: &WebState,
    scope: &ResolvedScope,
    record: &RuntimeProfileRecord,
    checked_at_ms: Option<i64>,
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> psychevo_runtime::Result<wire::RuntimeProfileView> {
    let config = &record.config;
    let fingerprint = runtime_profile_fingerprint(config);
    let revision = crate::runtime_profile_config_revision(&fingerprint);
    let backend = config
        .backend_ref
        .as_deref()
        .and_then(|backend_ref| backends.get(backend_ref));
    let health = runtime_profile_health_for_state(state, config, backend, checked_at_ms);
    let capabilities = match config.runtime {
        RuntimeProfileKind::Native => {
            ["turn.start", "turn.interrupt", "turn.steer", "history.read"]
                .into_iter()
                .map(|id| wire::RuntimeCapabilityView {
                    id: id.to_string(),
                    enabled: true,
                    stability: wire::RuntimeStabilityView::Stable,
                    unavailable_reason: None,
                })
                .collect()
        }
        RuntimeProfileKind::Acp => Vec::new(),
    };
    Ok(wire::RuntimeProfileView {
        id: config.id.clone(),
        runtime: config.runtime.as_str().to_string(),
        enabled: config.enabled,
        label: config.label.clone(),
        generated: record.generated,
        configured: !record.generated,
        backend_ref: config.backend_ref.clone(),
        provenance: match config.runtime {
            RuntimeProfileKind::Native => "Native".to_string(),
            RuntimeProfileKind::Acp => "ACP".to_string(),
        },
        profile_revision: revision.to_string(),
        capability_revision: revision.to_string(),
        stability: (config.runtime == RuntimeProfileKind::Native)
            .then_some(wire::RuntimeStabilityView::Stable),
        capabilities,
        default_model: config.default_model.clone(),
        default_mode: config.default_mode.clone(),
        default_agent: config.default_agent.clone(),
        approval_mode: config.approval_mode.clone(),
        sandbox: config.sandbox.clone(),
        workspace_roots: config.workspace_roots.clone(),
        option_keys: runtime_profile_option_keys(&config.options),
        source_targets: runtime_profile_source_targets(state, scope, &config.id)?,
        readiness_stages: runtime_readiness_stages(config, &health),
        health,
        diagnostics: runtime_profile_diagnostics(config, backend),
    })
}

fn runtime_profile_fingerprint(config: &RuntimeProfileConfig) -> String {
    crate::runtime_profile_config_fingerprint(config)
}

fn bound_runtime_profile_record(
    binding: &GatewayRuntimeBindingRecord,
) -> psychevo_runtime::Result<RuntimeProfileRecord> {
    let runtime_ref = binding.runtime_ref.as_deref().ok_or_else(|| {
        runtime_rpc_error(
            "bound_profile_snapshot_missing",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Resolved runtime binding is missing its Runtime Profile identity.".to_string(),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        )
    })?;
    let encoded = binding.profile_config_json.as_deref().ok_or_else(|| {
        runtime_rpc_error(
            "bound_profile_snapshot_missing",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound thread is missing its immutable effective Runtime Profile snapshot.".to_string(),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        )
    })?;
    let config: RuntimeProfileConfig = serde_json::from_str(encoded).map_err(|error| {
        runtime_rpc_error(
            "bound_profile_snapshot_invalid",
            "binding",
            wire::RuntimeRetryClassView::Never,
            format!("Bound Runtime Profile snapshot could not be decoded: {error}"),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        )
    })?;
    if config.id != runtime_ref {
        return Err(runtime_rpc_error(
            "bound_profile_snapshot_mismatch",
            "binding",
            wire::RuntimeRetryClassView::Never,
            format!(
                "Bound Runtime Profile snapshot identifies `{}`, but the binding identifies `{runtime_ref}`.",
                config.id
            ),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let fingerprint = runtime_profile_fingerprint(&config);
    if binding.profile_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(runtime_rpc_error(
            "bound_profile_snapshot_mismatch",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound Runtime Profile snapshot does not match its immutable fingerprint.".to_string(),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let revision = crate::runtime_profile_config_revision(&fingerprint);
    if binding
        .profile_revision
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|captured| captured != revision)
    {
        return Err(runtime_rpc_error(
            "bound_profile_snapshot_mismatch",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound Runtime Profile snapshot does not match its captured revision.".to_string(),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    Ok(RuntimeProfileRecord {
        config,
        generated: false,
    })
}

fn validate_bound_agent_snapshot(
    binding: &GatewayRuntimeBindingRecord,
) -> psychevo_runtime::Result<()> {
    let diagnostic_ref = Some(format!("agent-binding:{}", binding.thread_id));
    let fingerprint = binding.agent_fingerprint.as_deref().ok_or_else(|| {
        runtime_rpc_error(
            "bound_agent_snapshot_missing",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound thread is missing its immutable Agent Definition fingerprint.".to_string(),
            diagnostic_ref.clone(),
        )
    })?;
    let encoded = binding.agent_definition_json.as_deref().ok_or_else(|| {
        runtime_rpc_error(
            "bound_agent_snapshot_missing",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound thread is missing its immutable Agent Definition snapshot.".to_string(),
            diagnostic_ref.clone(),
        )
    })?;
    let snapshot: Value = serde_json::from_str(encoded).map_err(|error| {
        runtime_rpc_error(
            "bound_agent_snapshot_invalid",
            "binding",
            wire::RuntimeRetryClassView::Never,
            format!("Bound Agent Definition snapshot could not be decoded: {error}"),
            diagnostic_ref.clone(),
        )
    })?;
    if crate::gateway_agent_definition_fingerprint(encoded) != fingerprint {
        return Err(runtime_rpc_error(
            "bound_agent_snapshot_mismatch",
            "binding",
            wire::RuntimeRetryClassView::Never,
            "Bound Agent Definition snapshot does not match its immutable fingerprint.".to_string(),
            diagnostic_ref,
        ));
    }
    if let Some(agent_ref) = binding.agent_ref.as_deref()
        && snapshot.get("name").and_then(Value::as_str) != Some(agent_ref)
    {
        return Err(runtime_rpc_error(
            "bound_agent_snapshot_mismatch",
            "binding",
            wire::RuntimeRetryClassView::Never,
            format!(
                "Bound Agent Definition snapshot does not identify captured Agent `{agent_ref}`."
            ),
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    Ok(())
}

fn runtime_readiness_stages(
    config: &RuntimeProfileConfig,
    health: &wire::RuntimeHealthView,
) -> Vec<wire::RuntimeReadinessStageView> {
    let status = match health.status.as_str() {
        "ready" => wire::RuntimeReadinessStatusView::Ready,
        "missing" => wire::RuntimeReadinessStatusView::Missing,
        "needs_auth" => wire::RuntimeReadinessStatusView::NeedsAuth,
        "unsupported" | "disabled" => wire::RuntimeReadinessStatusView::Unsupported,
        "error" => wire::RuntimeReadinessStatusView::Error,
        _ => wire::RuntimeReadinessStatusView::Unchecked,
    };
    vec![wire::RuntimeReadinessStageView {
        id: match config.runtime {
            RuntimeProfileKind::Native => "runtime",
            RuntimeProfileKind::Acp => "backend",
        }
        .to_string(),
        status,
        summary: health.summary.clone(),
        observed_at_ms: health.checked_at_ms,
    }]
}

pub(super) fn runtime_profile_health(
    config: &RuntimeProfileConfig,
    backend: Option<&AgentBackendConfig>,
    checked_at_ms: Option<i64>,
) -> wire::RuntimeHealthView {
    if !config.enabled {
        return wire::RuntimeHealthView {
            status: "disabled".to_string(),
            summary: "Runtime Profile is disabled.".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    if config.runtime == RuntimeProfileKind::Native {
        return wire::RuntimeHealthView {
            status: "ready".to_string(),
            summary: "Native Psychevo Agent runtime is available.".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    let Some(backend_ref) = config.backend_ref.as_deref() else {
        return wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: "ACP Runtime Profile is missing backendRef.".to_string(),
            command_path: None,
            checked_at_ms,
        };
    };
    let Some(backend) = backend else {
        return wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: format!("ACP backend `{backend_ref}` is not configured."),
            command_path: None,
            checked_at_ms,
        };
    };
    if !backend.enabled {
        return wire::RuntimeHealthView {
            status: "disabled".to_string(),
            summary: format!("ACP backend `{backend_ref}` is disabled."),
            command_path: None,
            checked_at_ms,
        };
    }
    if backend
        .command
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: format!("ACP backend `{backend_ref}` has no launch command."),
            command_path: None,
            checked_at_ms,
        };
    }
    wire::RuntimeHealthView {
        status: "unchecked".to_string(),
        summary: format!(
            "ACP backend `{backend_ref}` is configured; readiness is established by Agent session negotiation."
        ),
        command_path: None,
        checked_at_ms,
    }
}

fn runtime_profile_health_for_state(
    state: &WebState,
    config: &RuntimeProfileConfig,
    backend: Option<&AgentBackendConfig>,
    checked_at_ms: Option<i64>,
) -> wire::RuntimeHealthView {
    let base = runtime_profile_health(config, backend, checked_at_ms);
    if config.backend_ref.as_deref() != Some(crate::managed_acp::CODEX_ACP_BACKEND_ID) {
        return base;
    }
    match crate::managed_acp::inspect_managed_codex_acp(&state.inner.home, HostPlatform::current())
    {
        crate::managed_acp::ManagedCodexAcpStatus::Ready(paths) => {
            if base.status != "unchecked" {
                base
            } else {
                wire::RuntimeHealthView {
                    status: "ready".to_string(),
                    summary: format!(
                        "Managed Codex ACP {} is installed and verified.",
                        crate::managed_acp::CODEX_ACP_VERSION
                    ),
                    command_path: Some(paths.executable.display().to_string()),
                    checked_at_ms,
                }
            }
        }
        crate::managed_acp::ManagedCodexAcpStatus::Missing { paths } => wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: "Managed Codex ACP is not installed; run backend/install.".to_string(),
            command_path: Some(paths.root.display().to_string()),
            checked_at_ms,
        },
        crate::managed_acp::ManagedCodexAcpStatus::Invalid { paths, reason } => {
            wire::RuntimeHealthView {
                status: "error".to_string(),
                summary: format!("{reason}; run backend/repair."),
                command_path: Some(paths.root.display().to_string()),
                checked_at_ms,
            }
        }
    }
}

fn ensure_managed_codex_profile_ready(
    state: &WebState,
    config: &RuntimeProfileConfig,
) -> psychevo_runtime::Result<()> {
    if config.runtime != RuntimeProfileKind::Acp
        || config.backend_ref.as_deref() != Some(crate::managed_acp::CODEX_ACP_BACKEND_ID)
    {
        return Ok(());
    }
    match crate::managed_acp::inspect_managed_codex_acp(&state.inner.home, HostPlatform::current())
    {
        crate::managed_acp::ManagedCodexAcpStatus::Ready(_) => Ok(()),
        crate::managed_acp::ManagedCodexAcpStatus::Missing { .. } => Err(agent_session_error(
            "managed_codex_acp_missing",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            "Managed Codex ACP is not installed; run backend/install before starting a turn.",
            Some("backend:codex".to_string()),
        )),
        crate::managed_acp::ManagedCodexAcpStatus::Invalid { reason, .. } => {
            Err(agent_session_error(
                "managed_codex_acp_invalid",
                AgentErrorStage::Configuration,
                "user_action",
                "not_delivered",
                format!("{reason}; run backend/repair before starting a turn."),
                Some("backend:codex".to_string()),
            ))
        }
    }
}

fn runtime_profile_diagnostics(
    config: &RuntimeProfileConfig,
    backend: Option<&AgentBackendConfig>,
) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !config.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "Runtime Profile is disabled.".to_string(),
        });
    }
    if config.runtime == RuntimeProfileKind::Acp {
        match config.backend_ref.as_deref() {
            None => diagnostics.push(wire::BackendDiagnosticView {
                kind: "missing_backend_ref".to_string(),
                message: "ACP Runtime Profiles require a backendRef.".to_string(),
            }),
            Some(backend_ref) if backend.is_none() => {
                diagnostics.push(wire::BackendDiagnosticView {
                    kind: "missing_backend".to_string(),
                    message: format!("ACP backend `{backend_ref}` is not configured."),
                });
            }
            Some(_) => {}
        }
    }
    diagnostics
}

fn runtime_profile_option_keys(options: &Value) -> Vec<String> {
    options
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn runtime_profile_config_options(
    config: &RuntimeProfileConfig,
) -> Vec<wire::RuntimeConfigOptionView> {
    [
        ("model", "Model", config.default_model.as_deref()),
        ("mode", "Mode", config.default_mode.as_deref()),
    ]
    .into_iter()
    .filter_map(|(id, label, current)| {
        let current = current?.trim();
        if current.is_empty() {
            return None;
        }
        Some(wire::RuntimeConfigOptionView {
            id: id.to_string(),
            name: label.to_string(),
            description: Some("Captured Runtime Profile default.".to_string()),
            category: Some(id.to_string()),
            option_type: "select".to_string(),
            current_value: Some(current.to_string()),
            values: vec![wire::RuntimeConfigOptionValueView {
                value: current.to_string(),
                name: current.to_string(),
                description: None,
                group: None,
            }],
        })
    })
    .collect()
}

struct AgentSurfaceDescriptor {
    controls: Vec<wire::ThreadControlDescriptorView>,
    input_capabilities: Vec<wire::ThreadInputCapabilityView>,
    capabilities: Vec<wire::RuntimeCapabilityView>,
    actions: Vec<wire::ThreadActionKind>,
    history: wire::ThreadHistoryView,
}

impl Default for AgentSurfaceDescriptor {
    fn default() -> Self {
        Self {
            controls: Vec::new(),
            input_capabilities: Vec::new(),
            capabilities: Vec::new(),
            actions: Vec::new(),
            history: unavailable_history(
                "History is unavailable until a valid Agent target is selected.",
            ),
        }
    }
}

fn profile_agent_surface_descriptor(
    config: &RuntimeProfileConfig,
    bound: bool,
    capability_revision: String,
    ready: bool,
    unavailable_reason: &str,
) -> AgentSurfaceDescriptor {
    match config.runtime {
        RuntimeProfileKind::Native => native_agent_surface_descriptor(
            config,
            bound,
            capability_revision,
            ready,
            unavailable_reason,
        ),
        RuntimeProfileKind::Acp => acp_profile_surface_descriptor(
            config,
            bound,
            capability_revision,
            ready,
            unavailable_reason,
        ),
    }
}

fn native_agent_surface_descriptor(
    config: &RuntimeProfileConfig,
    bound: bool,
    capability_revision: String,
    ready: bool,
    unavailable_reason: &str,
) -> AgentSurfaceDescriptor {
    let mode = native_runtime_mode_option();
    let descriptors = vec![
        (
            "model",
            "Model",
            wire::ThreadControlSurfaceRoleView::Model,
            config.default_model.clone(),
            Vec::new(),
        ),
        (
            "reasoning",
            "Reasoning",
            wire::ThreadControlSurfaceRoleView::Reasoning,
            None,
            REASONING_EFFORT_VALUES
                .iter()
                .map(|value| wire::ThreadControlChoiceView {
                    value: Value::String((*value).to_string()),
                    label: (*value).to_string(),
                    description: None,
                })
                .collect(),
        ),
        (
            "mode",
            "Mode",
            wire::ThreadControlSurfaceRoleView::Mode,
            config
                .default_mode
                .clone()
                .or_else(|| mode.current_value.clone()),
            mode.values
                .into_iter()
                .map(|choice| wire::ThreadControlChoiceView {
                    value: Value::String(choice.value),
                    label: choice.name,
                    description: choice.description,
                })
                .collect(),
        ),
        (
            "permissionMode",
            "Permission mode",
            wire::ThreadControlSurfaceRoleView::Advanced,
            Some("default".to_string()),
            ["default", "acceptEdits", "dontAsk", "bypassPermissions"]
                .into_iter()
                .map(|value| wire::ThreadControlChoiceView {
                    value: Value::String(value.to_string()),
                    label: value.to_string(),
                    description: None,
                })
                .collect(),
        ),
    ];
    let controls = descriptors
        .into_iter()
        .map(|(id, label, surface_role, effective, choices)| {
            let profile_default = match id {
                "model" => config.default_model.is_some(),
                "mode" => config.default_mode.is_some(),
                _ => false,
            };
            wire::ThreadControlDescriptorView {
                id: id.to_string(),
                label: label.to_string(),
                surface_role,
                mutability: wire::ThreadControlMutabilityView::Selectable,
                enabled: ready,
                required: id == "model",
                unavailable_reason: (!ready).then(|| unavailable_reason.to_string()),
                effective_value: effective.map(Value::String),
                effective_source: if profile_default {
                    wire::ThreadControlEffectiveSourceView::ProfileDefault
                } else {
                    wire::ThreadControlEffectiveSourceView::RuntimeDefault
                },
                is_default: true,
                choices,
                depends_on: None,
                apply_scope: if bound {
                    wire::ThreadControlApplyScopeView::Session
                } else {
                    wire::ThreadControlApplyScopeView::TurnDraft
                },
                stability: wire::RuntimeStabilityView::Stable,
                channel_safe: true,
                capability_revision: capability_revision.clone(),
            }
        })
        .collect();
    AgentSurfaceDescriptor {
        controls,
        input_capabilities: thread_input_capabilities(
            ready,
            unavailable_reason,
            |kind| matches!(kind, "text" | "image" | "embeddedContext" | "agentMention"),
            "Psychevo (Native) Adapter does not implement this input kind.",
        ),
        capabilities: [
            "turn.start",
            "turn.interrupt",
            "turn.steer",
            "context.compact",
            "history.read",
        ]
        .into_iter()
        .map(|id| {
            effective_capability_view(
                id,
                ready,
                true,
                thread_application_exposes_capability(id),
                (!ready).then(|| unavailable_reason.to_string()),
            )
        })
        .collect(),
        actions: vec![
            wire::ThreadActionKind::Interrupt,
            wire::ThreadActionKind::Steer,
            wire::ThreadActionKind::Compact,
            wire::ThreadActionKind::Fork,
            wire::ThreadActionKind::ForkBefore,
            wire::ThreadActionKind::RevertConversation,
            wire::ThreadActionKind::UnrevertConversation,
        ],
        history: if bound {
            wire::ThreadHistoryView {
                owner: wire::ThreadHistoryOwnerView::Psychevo,
                fidelity: wire::ThreadHistoryFidelityView::Full,
                cursor: None,
                hint: None,
            }
        } else {
            unavailable_history("History becomes available after the public Thread is bound.")
        },
    }
}

fn acp_profile_surface_descriptor(
    config: &RuntimeProfileConfig,
    bound: bool,
    capability_revision: String,
    ready: bool,
    unavailable_reason: &str,
) -> AgentSurfaceDescriptor {
    let options = runtime_profile_config_options(config);
    let controls = options
        .into_iter()
        .map(|option| wire::ThreadControlDescriptorView {
            surface_role: match option.id.as_str() {
                "model" => wire::ThreadControlSurfaceRoleView::Model,
                "mode" | "agent" => wire::ThreadControlSurfaceRoleView::Mode,
                _ => wire::ThreadControlSurfaceRoleView::Advanced,
            },
            id: option.id,
            label: option.name,
            mutability: wire::ThreadControlMutabilityView::Selectable,
            enabled: ready,
            required: false,
            unavailable_reason: (!ready).then(|| unavailable_reason.to_string()),
            effective_value: option.current_value.map(Value::String),
            effective_source: wire::ThreadControlEffectiveSourceView::ProfileDefault,
            is_default: true,
            choices: option
                .values
                .into_iter()
                .map(|choice| wire::ThreadControlChoiceView {
                    value: Value::String(choice.value),
                    label: choice.name,
                    description: choice.description,
                })
                .collect(),
            depends_on: None,
            apply_scope: if bound {
                wire::ThreadControlApplyScopeView::Session
            } else {
                wire::ThreadControlApplyScopeView::TurnDraft
            },
            stability: wire::RuntimeStabilityView::Stable,
            channel_safe: true,
            capability_revision: capability_revision.clone(),
        })
        .collect();
    AgentSurfaceDescriptor {
        controls,
        // Text is a baseline stable-v1 prompt block. Optional ACP input kinds
        // stay disabled until a resident session publishes negotiated facts.
        input_capabilities: thread_input_capabilities(
            ready,
            unavailable_reason,
            |kind| kind == "text",
            "This optional ACP input kind is not negotiated until the Agent session is attached.",
        ),
        capabilities: vec![effective_capability_view(
            "turn.start",
            ready,
            true,
            true,
            (!ready).then(|| unavailable_reason.to_string()),
        )],
        actions: vec![wire::ThreadActionKind::Interrupt],
        history: if bound {
            wire::ThreadHistoryView {
                owner: wire::ThreadHistoryOwnerView::Process,
                fidelity: wire::ThreadHistoryFidelityView::Partial,
                cursor: None,
                hint: Some(
                    "ACP history authority is finalized from the resident Agent session snapshot."
                        .to_string(),
                ),
            }
        } else {
            unavailable_history("History becomes available after the public Thread is bound.")
        },
    }
}

fn unavailable_history(hint: &str) -> wire::ThreadHistoryView {
    wire::ThreadHistoryView {
        owner: wire::ThreadHistoryOwnerView::Psychevo,
        fidelity: wire::ThreadHistoryFidelityView::Unavailable,
        cursor: None,
        hint: Some(hint.to_string()),
    }
}

const THREAD_APPLICATION_INPUT_KINDS: [&str; 6] = [
    "text",
    "image",
    "resource",
    "resourceLink",
    "embeddedContext",
    "agentMention",
];

fn thread_input_capabilities(
    ready: bool,
    readiness_reason: &str,
    adapter_implements: impl Fn(&str) -> bool,
    adapter_reason: &str,
) -> Vec<wire::ThreadInputCapabilityView> {
    THREAD_APPLICATION_INPUT_KINDS
        .into_iter()
        .map(|kind| {
            let implemented = adapter_implements(kind);
            let enabled = ready && implemented;
            wire::ThreadInputCapabilityView {
                kind: kind.to_string(),
                enabled,
                unavailable_reason: (!enabled).then(|| {
                    if !ready {
                        readiness_reason.to_string()
                    } else {
                        adapter_reason.to_string()
                    }
                }),
            }
        })
        .collect()
}

fn thread_application_exposes_capability(id: &str) -> bool {
    matches!(
        id,
        "turn.start"
            | "turn.interrupt"
            | "turn.steer"
            | "context.compact"
            | "history.read"
            | "mcp.http"
            | "pack.codex"
            | "pack.opencode"
            | "codex.goal"
            | "codex.fastMode"
    ) || id.starts_with("command:")
}

fn effective_capability_view(
    id: &str,
    negotiated: bool,
    adapter_implemented: bool,
    application_exposed: bool,
    negotiated_reason: Option<String>,
) -> wire::RuntimeCapabilityView {
    let enabled = negotiated && adapter_implemented && application_exposed;
    let unavailable_reason = (!enabled).then(|| {
        if !negotiated {
            negotiated_reason.unwrap_or_else(|| format!("The Agent did not negotiate `{id}`."))
        } else if !adapter_implemented {
            format!("The selected Adapter does not implement `{id}`.")
        } else {
            format!("ThreadApplication does not expose `{id}`.")
        }
    });
    wire::RuntimeCapabilityView {
        id: id.to_string(),
        enabled,
        stability: wire::RuntimeStabilityView::Stable,
        unavailable_reason,
    }
}

fn acp_runtime_control_descriptors(
    options: Vec<wire::RuntimeConfigOptionView>,
    capability_revision: String,
) -> Vec<wire::ThreadControlDescriptorView> {
    options
        .into_iter()
        .map(|option| {
            let surface_role = match (
                option.id.as_str(),
                option.category.as_deref().unwrap_or_default(),
            ) {
                ("model", _) | (_, "model") => wire::ThreadControlSurfaceRoleView::Model,
                ("effort" | "variant", _) | (_, "thought_level") => {
                    wire::ThreadControlSurfaceRoleView::Reasoning
                }
                ("mode" | "agent", _) | (_, "mode") => wire::ThreadControlSurfaceRoleView::Mode,
                _ => wire::ThreadControlSurfaceRoleView::Advanced,
            };
            let effective_value = option.current_value.as_ref().map(|value| {
                if option.option_type == "boolean" {
                    value
                        .parse::<bool>()
                        .map(Value::Bool)
                        .unwrap_or_else(|_| Value::String(value.clone()))
                } else {
                    Value::String(value.clone())
                }
            });
            wire::ThreadControlDescriptorView {
                id: option.id,
                label: option.name,
                surface_role,
                mutability: wire::ThreadControlMutabilityView::Selectable,
                enabled: true,
                required: false,
                unavailable_reason: None,
                effective_value,
                effective_source: wire::ThreadControlEffectiveSourceView::RuntimeObserved,
                is_default: false,
                choices: option
                    .values
                    .into_iter()
                    .map(|choice| wire::ThreadControlChoiceView {
                        value: Value::String(choice.value),
                        label: choice.name,
                        description: choice.description,
                    })
                    .collect(),
                depends_on: None,
                apply_scope: wire::ThreadControlApplyScopeView::Session,
                stability: wire::RuntimeStabilityView::Stable,
                channel_safe: matches!(
                    surface_role,
                    wire::ThreadControlSurfaceRoleView::Model
                        | wire::ThreadControlSurfaceRoleView::Reasoning
                        | wire::ThreadControlSurfaceRoleView::Mode
                ),
                capability_revision: capability_revision.clone(),
            }
        })
        .collect()
}

pub(super) fn acp_session_mode_control_descriptor(
    modes: &[crate::acp_peer::AcpSessionModeSnapshot],
    current_mode_id: Option<&str>,
    capability_revision: String,
) -> Option<wire::ThreadControlDescriptorView> {
    if modes.is_empty() {
        return None;
    }
    Some(wire::ThreadControlDescriptorView {
        id: "mode".to_string(),
        label: "Mode".to_string(),
        surface_role: wire::ThreadControlSurfaceRoleView::Mode,
        mutability: wire::ThreadControlMutabilityView::Selectable,
        enabled: true,
        required: false,
        unavailable_reason: None,
        effective_value: current_mode_id.map(|mode| Value::String(mode.to_string())),
        effective_source: wire::ThreadControlEffectiveSourceView::RuntimeObserved,
        is_default: false,
        choices: modes
            .iter()
            .map(|mode| wire::ThreadControlChoiceView {
                value: Value::String(mode.id.clone()),
                label: mode.name.clone(),
                description: mode.description.clone(),
            })
            .collect(),
        depends_on: None,
        apply_scope: wire::ThreadControlApplyScopeView::Session,
        stability: wire::RuntimeStabilityView::Stable,
        channel_safe: true,
        capability_revision,
    })
}

fn acp_session_agent_surface_descriptor(
    snapshot: &crate::acp_peer::AcpSessionSnapshot,
    capability_revision: String,
) -> AgentSurfaceDescriptor {
    let mut controls =
        acp_runtime_control_descriptors(snapshot.options.clone(), capability_revision.clone());
    if !controls.iter().any(|control| control.id == "mode")
        && let Some(mode) = acp_session_mode_control_descriptor(
            &snapshot.available_modes,
            snapshot.current_mode_id.as_deref(),
            capability_revision,
        )
    {
        controls.push(mode);
    }
    let input_capabilities = THREAD_APPLICATION_INPUT_KINDS
        .into_iter()
        .map(|kind| {
            let negotiated = if kind == "agentMention" {
                false
            } else {
                snapshot.supports_input_kind(kind).unwrap_or(false)
            };
            let adapter_implemented = matches!(
                kind,
                "text" | "image" | "resource" | "resourceLink" | "embeddedContext"
            );
            let enabled = negotiated && adapter_implemented;
            wire::ThreadInputCapabilityView {
                kind: kind.to_string(),
                enabled,
                unavailable_reason: (!enabled).then(|| {
                    if kind == "agentMention" {
                        "Outbound ACP does not expose Psychevo top-level Agent mentions as a typed prompt capability."
                            .to_string()
                    } else if !negotiated {
                        format!("The ACP Agent did not negotiate `{kind}` input.")
                    } else {
                        format!("The ACP Adapter does not implement `{kind}` input.")
                    }
                }),
            }
        })
        .collect();
    let mut capabilities = vec![
        effective_capability_view("turn.start", true, true, true, None),
        effective_capability_view("turn.interrupt", true, true, true, None),
        effective_capability_view("history.read", true, true, true, None),
        effective_capability_view(
            "history.load",
            snapshot.capabilities.session.load,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "history.resume",
            snapshot.capabilities.session.resume,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "session.list",
            snapshot.capabilities.session.list,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "session.delete",
            snapshot.capabilities.session.delete,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "session.fork",
            snapshot.capabilities.session.fork,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "session.close",
            snapshot.capabilities.session.close,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "session.additionalDirectories",
            snapshot.capabilities.session.additional_directories,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "auth.logout",
            snapshot.capabilities.auth_logout,
            true,
            false,
            None,
        ),
        effective_capability_view(
            "providers.configure",
            snapshot.capabilities.providers,
            true,
            false,
            None,
        ),
        effective_capability_view("mcp.http", snapshot.capabilities.mcp_http, true, true, None),
        effective_capability_view("mcp.sse", snapshot.capabilities.mcp_sse, false, false, None),
        effective_capability_view("mcp.acp", snapshot.capabilities.mcp_acp, false, false, None),
    ];
    capabilities.extend(snapshot.capabilities.auth_methods.iter().map(|method| {
        effective_capability_view(
            &format!("auth.method:{}", method.id),
            true,
            true,
            false,
            None,
        )
    }));
    capabilities.extend(snapshot.available_commands.iter().map(|command| {
        effective_capability_view(&format!("command:{}", command.name), true, true, true, None)
    }));
    if let Some(pack) = crate::acp_peer::project_acp_capability_pack(snapshot) {
        capabilities.extend(pack.facts.into_iter().map(|fact| {
            let application_exposed = thread_application_exposes_capability(&fact.id);
            effective_capability_view(
                &fact.id,
                fact.enabled,
                true,
                application_exposed,
                fact.unavailable_reason,
            )
        }));
    }
    let mut actions = vec![wire::ThreadActionKind::Interrupt];
    if snapshot.capabilities.session.fork {
        actions.push(wire::ThreadActionKind::Fork);
    }
    AgentSurfaceDescriptor {
        controls,
        input_capabilities,
        capabilities,
        actions,
        history: wire::ThreadHistoryView {
            owner: match snapshot.history.owner {
                crate::acp_peer::AcpHistoryOwnerSnapshot::Agent => {
                    wire::ThreadHistoryOwnerView::Agent
                }
                crate::acp_peer::AcpHistoryOwnerSnapshot::Process => {
                    wire::ThreadHistoryOwnerView::Process
                }
            },
            fidelity: if snapshot.history.replay_complete {
                wire::ThreadHistoryFidelityView::Full
            } else {
                wire::ThreadHistoryFidelityView::Partial
            },
            cursor: None,
            hint: if !snapshot.history.resumable {
                Some(
                    "This ACP Agent history is process-ephemeral and cannot be resumed after restart."
                        .to_string(),
                )
            } else if snapshot.history.loaded_from_agent && !snapshot.history.replay_complete {
                Some(
                    "ACP Agent history replay is incomplete because some content lacked stable identity or exceeded product projection limits."
                        .to_string(),
                )
            } else if !snapshot.history.loaded_from_agent {
                Some(
                    "History is Agent-authoritative and resumable; this process has not loaded a prior session."
                        .to_string(),
                )
            } else {
                None
            },
        },
    }
}

fn apply_control_state_precedence(
    controls: &mut [wire::ThreadControlDescriptorView],
    binding: Option<&GatewayRuntimeBindingRecord>,
    source_lane: Option<&psychevo_runtime::GatewaySourceLaneRecord>,
) {
    for control in controls {
        if let Some(value) = binding
            .and_then(|binding| binding.thread_preferences.get(&control.id))
            .cloned()
        {
            control.effective_value = Some(value);
            control.effective_source = wire::ThreadControlEffectiveSourceView::ThreadPreference;
            control.is_default = false;
            continue;
        }
        if binding.is_none()
            && let Some(value) = source_lane
                .and_then(|lane| lane.draft_control_values.get(&control.id))
                .cloned()
        {
            control.effective_value = Some(Value::String(value));
            control.effective_source = wire::ThreadControlEffectiveSourceView::SourceDraft;
            control.is_default = false;
            continue;
        }
        if control.effective_value.is_none()
            && let Some(value) = binding
                .and_then(|binding| binding.runtime_observed.get(&control.id))
                .cloned()
        {
            control.effective_value = Some(value);
            control.effective_source = wire::ThreadControlEffectiveSourceView::RuntimeObserved;
            control.is_default = false;
        }
    }
}

fn populate_native_control_catalog(
    options: &RunOptions,
    configured: &[psychevo_runtime::ConfiguredModel],
    controls: &mut [wire::ThreadControlDescriptorView],
) {
    if let Some(model_control) = controls.iter_mut().find(|control| control.id == "model") {
        model_control.choices = configured
            .iter()
            .map(|model| {
                let value = format!("{}/{}", model.provider, model.model);
                wire::ThreadControlChoiceView {
                    value: Value::String(value.clone()),
                    label: model.model_name.clone().unwrap_or(value),
                    description: Some(model.provider_label.clone()),
                }
            })
            .collect();
        if model_control.effective_value.is_none()
            && let Ok(Some(model)) = selected_configured_model(options)
        {
            model_control.effective_value =
                Some(Value::String(format!("{}/{}", model.provider, model.model)));
            model_control.effective_source = wire::ThreadControlEffectiveSourceView::RuntimeDefault;
        }
    }
    if let Some(reasoning_control) = controls
        .iter_mut()
        .find(|control| control.id == "reasoning")
        && reasoning_control.effective_value.is_none()
        && let Some(reasoning) = options.reasoning_effort.clone().or_else(|| {
            selected_configured_model(options)
                .ok()
                .flatten()
                .and_then(|model| model.reasoning_effort)
        })
    {
        reasoning_control.effective_value = Some(Value::String(reasoning));
        reasoning_control.effective_source = wire::ThreadControlEffectiveSourceView::RuntimeDefault;
    }
}

fn decorate_configured_model_control_labels(
    configured: &[psychevo_runtime::ConfiguredModel],
    controls: &mut [wire::ThreadControlDescriptorView],
) {
    let Some(model_control) = controls
        .iter_mut()
        .find(|control| control.surface_role == wire::ThreadControlSurfaceRoleView::Model)
    else {
        return;
    };
    for choice in &mut model_control.choices {
        let Value::String(value) = &choice.value else {
            continue;
        };
        if let Some(name) = configured.iter().find_map(|model| {
            (format!("{}/{}", model.provider, model.model) == *value)
                .then(|| model.model_name.clone())
                .flatten()
        }) {
            choice.label = name;
        }
    }
}

fn source_draft_control_revision(
    source_lane: Option<&psychevo_runtime::GatewaySourceLaneRecord>,
    capability_revision: &str,
) -> String {
    let Some(source_lane) = source_lane else {
        return capability_revision.to_string();
    };
    if source_lane.draft_control_values.is_empty() {
        return capability_revision.to_string();
    }
    let encoded = serde_json::to_string(&source_lane.draft_control_values).unwrap_or_default();
    format!("{capability_revision}:{}", stable_hash_hex(&encoded))
}

pub(super) fn combined_thread_revision(parts: &[&str]) -> String {
    stable_hash_hex(&parts.join("\0"))
}

pub(super) fn apply_thread_control_precedence(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    options: &mut BTreeMap<String, String>,
) -> psychevo_runtime::Result<()> {
    if let Some(thread_id) = thread_id
        && let Some(binding) = state
            .inner
            .state
            .store()
            .gateway_runtime_binding(thread_id)?
    {
        for (control_id, value) in binding.thread_preferences {
            options.insert(control_id, thread_control_override_string_value(&value)?);
        }
        return Ok(());
    }
    if let Some(lane) = state
        .inner
        .state
        .store()
        .gateway_source_lane(&scope.source.source_key().0)?
    {
        options.extend(lane.draft_control_values);
    }
    Ok(())
}

pub(super) fn thread_control_override_string_value(
    value: &Value,
) -> psychevo_runtime::Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(agent_session_error(
            "invalid_control",
            AgentErrorStage::Control,
            "user_action",
            "not_delivered",
            "Thread control values must be strings, booleans, or numbers.",
            None,
        )),
    }
}

fn runtime_profile_config_json(
    params: &wire::RuntimeProfileWriteParams,
) -> psychevo_runtime::Result<Value> {
    validate_runtime_profile_kind(&params.runtime)?;
    let backend_ref = params
        .backend_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let runtime_kind = match params.runtime.trim() {
        "native" => RuntimeProfileKind::Native,
        "acp" => RuntimeProfileKind::Acp,
        _ => unreachable!("runtime kind was validated"),
    };
    psychevo_runtime::validate_runtime_profile_backend_ref(&params.id, runtime_kind, backend_ref)?;
    let mut object = serde_json::Map::new();
    object.insert("runtime".to_string(), json!(params.runtime.trim()));
    object.insert("enabled".to_string(), json!(params.enabled.unwrap_or(true)));
    if let Some(label) = params
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("label".to_string(), json!(label));
    }
    insert_optional_string(&mut object, "backend_ref", params.backend_ref.as_deref());
    insert_optional_string(
        &mut object,
        "default_model",
        params.default_model.as_deref(),
    );
    insert_optional_string(&mut object, "default_mode", params.default_mode.as_deref());
    insert_optional_string(
        &mut object,
        "default_agent",
        params.default_agent.as_deref(),
    );
    insert_optional_string(
        &mut object,
        "approval_mode",
        params.approval_mode.as_deref(),
    );
    insert_optional_string(&mut object, "sandbox", params.sandbox.as_deref());
    object.insert(
        "workspace_roots".to_string(),
        json!(trimmed_string_list(&params.workspace_roots)),
    );
    if let Some(options) = params.options.clone().filter(|value| !value.is_null()) {
        object.insert("options".to_string(), options);
    }
    Ok(Value::Object(object))
}

fn insert_optional_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        object.insert(key.to_string(), json!(value));
    }
}

fn validate_runtime_profile_kind(value: &str) -> psychevo_runtime::Result<()> {
    match value.trim() {
        "native" | "acp" => Ok(()),
        "codex" | "opencode" => Err(Error::Config(format!(
            "adapter_removed: runtime profile kind `{}` was removed; configure an ACP backend and use runtime = \"acp\" with backend_ref",
            value.trim()
        ))),
        _ => Err(Error::Message(format!(
            "runtime profile kind `{value}` must be native or acp"
        ))),
    }
}

fn runtime_profile_config_dir(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> PathBuf {
    match target {
        wire::BackendConfigTarget::Project => scope.cwd.join(".psychevo"),
        wire::BackendConfigTarget::Profile => active_profile_config_dir(state, scope),
    }
}

fn ensure_profile_config_for_runtime_profile_write(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<()> {
    if target != wire::BackendConfigTarget::Profile
        || !state
            .inner
            .inherited_env
            .get("PSYCHEVO_CONFIG")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    {
        return Ok(());
    }
    let config_path = active_profile_config_dir(state, scope).join("config.toml");
    if config_path.exists() {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, "")?;
    Ok(())
}

fn runtime_profile_source_targets(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigTarget>> {
    let mut targets = Vec::new();
    if runtime_profile_exists_in_config_dir(&active_profile_config_dir(state, scope), id)? {
        targets.push(wire::BackendConfigTarget::Profile);
    }
    if runtime_profile_exists_in_config_dir(&scope.cwd.join(".psychevo"), id)? {
        targets.push(wire::BackendConfigTarget::Project);
    }
    Ok(targets)
}

fn runtime_profile_exists_in_config_dir(
    config_dir: &Path,
    id: &str,
) -> psychevo_runtime::Result<bool> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    Ok(parsed
        .get("runtime_profiles")
        .or_else(|| parsed.get("runtimeProfiles"))
        .and_then(|value| value.get(id))
        .is_some())
}

fn trimmed_string_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod runtime_session_ownership_tests {
    use super::*;

    fn ephemeral_web_state() -> (tempfile::TempDir, WebState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let env = BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ]);
        let runtime_state =
            StateRuntime::open(temp.path().join("state.db")).expect("state runtime");
        let gateway = Gateway::new(runtime_state);
        let config =
            GatewayWebServerConfig::new(gateway, home, cwd, None, env, temp.path().join("static"));
        (temp, WebState::new(config))
    }

    #[test]
    fn runnable_target_catalog_reuses_one_snapshot_until_invalidated() {
        let (_temp, state) = ephemeral_web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let first = RunnableTargetCatalog::load(&state, &scope).expect("first catalog");
        let second = RunnableTargetCatalog::load(&state, &scope).expect("cached catalog");
        assert!(Arc::ptr_eq(&first, &second));

        state.invalidate_runnable_target_catalog();
        let refreshed = RunnableTargetCatalog::load(&state, &scope).expect("refreshed catalog");
        assert!(!Arc::ptr_eq(&first, &refreshed));
    }

    #[test]
    fn prospective_acp_context_uses_configured_name_for_raw_model_choice() {
        let (_temp, state) = ephemeral_web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"[provider.test.models.default]
name = "Configured default"

[agents.backends.fixture]
kind = "acp"
command = "fixture-agent"
entrypoints = ["peer", "subagent"]

[runtime_profiles.fixture]
runtime = "acp"
backend_ref = "fixture"
default_model = "test/default"
"#,
        )
        .expect("config");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let context = thread_context_read_result(
            &state,
            &scope,
            wire::ThreadContextReadParams {
                thread_id: None,
                target: Some(wire::RunnableTargetInput {
                    agent_ref: Some("fixture".to_string()),
                    runtime_profile_ref: "fixture".to_string(),
                }),
                scope: Some(scope.to_wire_scope()),
            },
        )
        .expect("prospective ACP Thread Context");
        let model = context
            .controls
            .iter()
            .find(|control| control.surface_role == wire::ThreadControlSurfaceRoleView::Model)
            .expect("model control");
        assert_eq!(model.effective_value, Some(json!("test/default")));
        assert_eq!(model.choices[0].label, "Configured default");
    }

    #[cfg(unix)]
    #[test]
    fn thread_context_catalog_read_does_not_materialize_path_backends() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&bin).expect("bin");
        let codex = bin.join("codex");
        std::fs::write(&codex, "#!/bin/sh\nexit 0\n").expect("codex fixture");
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755))
            .expect("codex fixture permissions");
        let env = BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
            ("PATH".to_string(), bin.to_string_lossy().to_string()),
        ]);
        let runtime_state =
            StateRuntime::open(temp.path().join("state.db")).expect("state runtime");
        let gateway = Gateway::new(runtime_state);
        let state = WebState::new(GatewayWebServerConfig::new(
            gateway,
            home.clone(),
            cwd,
            None,
            env,
            temp.path().join("static"),
        ));
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        thread_context_read_result(
            &state,
            &scope,
            wire::ThreadContextReadParams {
                thread_id: None,
                target: None,
                scope: Some(scope.to_wire_scope()),
            },
        )
        .expect("cache-only Thread Context");

        assert!(
            !home.join("config.toml").exists(),
            "Thread Context must not turn PATH discovery into persistent configuration"
        );
    }

    #[tokio::test]
    async fn process_ephemeral_restart_keeps_cached_context_and_requires_a_new_thread() {
        let (temp, state) = ephemeral_web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        let script_path = temp.path().join("process_ephemeral_acp.py");
        let log_path = temp.path().join("process_ephemeral_methods.log");
        std::fs::write(
            &script_path,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/process_ephemeral_acp.py"
            )),
        )
        .expect("ACP fixture");
        let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let python = psychevo_runtime::resolve_executable_path(
            "python3",
            &state.inner.cwd,
            &psychevo_runtime::ExecutableResolveOptions {
                platform: HostPlatform::current(),
                env: &host_env,
            },
        )
        .expect("python3");
        std::fs::write(
            state.inner.home.join("config.toml"),
            format!(
                r#"[agents.backends.ephemeral]
kind = "acp"
label = "Ephemeral"
command = {}
args = [{}, {}]
entrypoints = ["peer"]

[runtime_profiles.ephemeral]
runtime = "acp"
enabled = true
label = "Ephemeral ACP"
backend_ref = "ephemeral"
"#,
                serde_json::to_string(&python.to_string_lossy()).expect("python path"),
                serde_json::to_string(&script_path.to_string_lossy()).expect("script path"),
                serde_json::to_string(&log_path.to_string_lossy()).expect("log path"),
            ),
        )
        .expect("config");
        let profile = RuntimeProfileConfig {
            id: "ephemeral".to_string(),
            runtime: RuntimeProfileKind::Acp,
            enabled: true,
            label: "Ephemeral ACP".to_string(),
            backend_ref: Some("ephemeral".to_string()),
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        };
        let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
        let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
        let profile_revision =
            crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
        let agent_json = r#"{"name":"ephemeral","instructions":"captured"}"#;
        let agent_fingerprint = crate::gateway_agent_definition_fingerprint(agent_json);
        let thread_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.cwd, "web", "pending", "pending", None)
            .expect("thread");
        let cwd = state.inner.cwd.display().to_string();
        state
            .inner
            .state
            .store()
            .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
                thread_id: &thread_id,
                agent_ref: Some("ephemeral"),
                agent_fingerprint: &agent_fingerprint,
                agent_definition_json: agent_json,
                runtime_ref: "ephemeral",
                backend_kind: "acp",
                native_kind: "acp",
                native_session_id: Some("ephemeral-native-1"),
                cwd: &cwd,
                profile_fingerprint: &profile_fingerprint,
                profile_revision: &profile_revision,
                profile_config_json: &profile_json,
                adapter_kind: "acp",
                adapter_revision: "test",
                ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                parent_thread_id: None,
            })
            .expect("binding");
        let persisted_projection = crate::acp_peer::AcpSessionSnapshot {
            native_session_id: "ephemeral-native-1".to_string(),
            agent: Some(crate::acp_peer::AcpAgentIdentitySnapshot {
                name: "ephemeral-test".to_string(),
                title: Some("Ephemeral".to_string()),
                version: "1.0.0".to_string(),
            }),
            capabilities: crate::acp_peer::AcpNegotiatedCapabilitiesSnapshot {
                prompt_input: crate::acp_peer::AcpPromptInputCapabilitiesSnapshot {
                    text: true,
                    image: false,
                    audio: false,
                    resource: false,
                    resource_link: false,
                    embedded_context: true,
                },
                session: crate::acp_peer::AcpSessionLifecycleCapabilitiesSnapshot {
                    load: false,
                    list: false,
                    delete: false,
                    fork: false,
                    resume: false,
                    close: false,
                    additional_directories: false,
                },
                auth_logout: false,
                auth_methods: Vec::new(),
                providers: false,
                mcp_http: false,
                mcp_sse: false,
                mcp_acp: false,
            },
            options: Vec::new(),
            available_commands: Vec::new(),
            available_modes: Vec::new(),
            current_mode_id: None,
            legacy_models: None,
            history: crate::acp_peer::AcpHistorySnapshot {
                owner: crate::acp_peer::AcpHistoryOwnerSnapshot::Process,
                resumable: false,
                load_supported: false,
                resume_supported: false,
                loaded_from_agent: false,
                replay_complete: true,
                replay_update_count: 0,
                live_update_count: 0,
            },
            session_info: crate::acp_peer::AcpSessionInfoSnapshot::default(),
            generation: 1,
            session_epoch: 1,
            control_revision: "ephemeral-controls".to_string(),
            projection_revision: "ephemeral-projection".to_string(),
        };
        state
            .inner
            .state
            .store()
            .set_session_metadata_field(
                &thread_id,
                ACP_PEER_METADATA_KEY,
                Some(json!({
                    "agentName": "ephemeral",
                    "backendId": "ephemeral",
                    "backendKind": "acp",
                    "nativeSessionId": "ephemeral-native-1",
                    "sessionProjection": persisted_projection,
                })),
            )
            .expect("persist ACP product projection");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let context = thread_context_read_result_live(
            &state,
            &scope,
            wire::ThreadContextReadParams {
                thread_id: Some(thread_id),
                target: None,
                scope: Some(scope.to_wire_scope()),
            },
        )
        .await
        .expect("process-ephemeral cached Thread Context");

        assert_eq!(context.history.owner, wire::ThreadHistoryOwnerView::Process);
        assert_eq!(
            context.history.fidelity,
            wire::ThreadHistoryFidelityView::Partial
        );
        assert!(
            context
                .history
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("Start a new Thread"))
        );
        assert!(!context.sendability.allowed);
        assert_eq!(
            context.sendability.recovery_action.as_deref(),
            Some("thread/draft/open")
        );
        assert!(
            context
                .sendability
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("process-ephemeral"))
        );
        assert!(
            !log_path.exists(),
            "cache-only Thread Context must not initialize, load, or resume the ACP Agent"
        );
    }

    #[test]
    fn source_profile_without_agent_resolves_the_acp_backend_target() {
        let (_temp, state) = ephemeral_web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"[agents.backends.reviewer]
kind = "acp"
label = "Reviewer"
command = "/bin/true"
entrypoints = ["peer"]

[runtime_profiles.reviewer]
runtime = "acp"
enabled = true
label = "Reviewer ACP"
backend_ref = "reviewer"
"#,
        )
        .expect("config");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

        let target = runnable_target_for_source(&state, &scope, &scope.source, "reviewer")
            .expect("ACP source target");

        assert_eq!(target.runtime_profile_ref, "reviewer");
        assert_eq!(target.agent_ref.as_deref(), Some("reviewer"));
        assert_eq!(target.agent_label, "reviewer");
    }

    #[test]
    fn generated_public_profiles_use_only_native_and_acp() {
        let profiles = generated_runtime_profiles();
        assert_eq!(
            profiles
                .iter()
                .map(|profile| {
                    (
                        profile.id.as_str(),
                        profile.runtime,
                        profile.backend_ref.as_deref(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("native", RuntimeProfileKind::Native, None),
                ("codex", RuntimeProfileKind::Acp, Some("codex")),
                ("opencode", RuntimeProfileKind::Acp, Some("opencode")),
            ]
        );
    }

    #[test]
    fn retired_direct_runtime_kinds_fail_with_adapter_removed() {
        for runtime in ["codex", "opencode"] {
            let error = validate_runtime_profile_kind(runtime).expect_err("retired kind");
            assert!(error.to_string().contains("adapter_removed"));
        }
    }

    #[test]
    fn runtime_profile_write_keeps_launch_configuration_on_the_backend() {
        let value = runtime_profile_config_json(&wire::RuntimeProfileWriteParams {
            id: "reviewer".to_string(),
            target: wire::BackendConfigTarget::Project,
            runtime: "acp".to_string(),
            enabled: Some(true),
            label: Some("Reviewer".to_string()),
            backend_ref: Some("reviewer-agent".to_string()),
            default_model: Some("model-a".to_string()),
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: None,
            scope: None,
        })
        .expect("ACP profile config");
        let object = value.as_object().expect("profile object");
        assert_eq!(object.get("runtime"), Some(&json!("acp")));
        assert_eq!(object.get("backend_ref"), Some(&json!("reviewer-agent")));
        for launch_field in ["command", "args", "env", "cwd"] {
            assert!(!object.contains_key(launch_field));
        }
    }

    #[test]
    fn public_target_revisions_ignore_secret_values_but_track_structural_capabilities() {
        let agent_a = json!({
            "name": "reviewer",
            "enabled": true,
            "source": "project",
            "instructions": "secret instruction alpha",
            "initial_prompt": "private initial prompt alpha",
            "project_instructions": true,
            "model": "provider/model-a",
            "background": false,
            "max_turns": 8,
            "max_spawn_depth": 2,
            "effort": "medium",
            "entrypoints": ["peer"],
            "skills": ["review"],
            "optional_contributions": ["mcp"],
            "tool_policy": {
                "allowed": ["Read"],
                "denied": [],
                "allowed_agents": null,
                "denied_agents": [],
                "permission_mode": "default",
                "mcp_servers": ["source"]
            }
        });
        let mut agent_secret_changed = agent_a.clone();
        agent_secret_changed["instructions"] = json!("secret instruction beta");
        agent_secret_changed["initial_prompt"] = json!("private initial prompt beta");
        assert_eq!(
            stable_hash_hex(&public_redacted_agent_structure(&agent_a).to_string()),
            stable_hash_hex(&public_redacted_agent_structure(&agent_secret_changed).to_string()),
            "private instructions must not become a public equality oracle"
        );
        let mut agent_capability_changed = agent_a.clone();
        agent_capability_changed["tool_policy"]["mcp_servers"] = json!(["source", "review-db"]);
        assert_ne!(
            stable_hash_hex(&public_redacted_agent_structure(&agent_a).to_string()),
            stable_hash_hex(
                &public_redacted_agent_structure(&agent_capability_changed).to_string()
            ),
            "structural capability names must revise Thread Context"
        );
        let mut agent_execution_shape_changed = agent_a.clone();
        agent_execution_shape_changed["model"] = json!("provider/model-b");
        agent_execution_shape_changed["project_instructions"] = json!(false);
        assert_ne!(
            stable_hash_hex(&public_redacted_agent_structure(&agent_a).to_string()),
            stable_hash_hex(
                &public_redacted_agent_structure(&agent_execution_shape_changed).to_string()
            ),
            "non-secret execution policy changes must revise Thread Context"
        );

        let profile = RuntimeProfileConfig {
            id: "reviewer".to_string(),
            runtime: RuntimeProfileKind::Acp,
            enabled: true,
            label: "Reviewer".to_string(),
            backend_ref: Some("reviewer".to_string()),
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: json!({"privateToken": "alpha"}),
        };
        let mut option_secret_changed = profile.clone();
        option_secret_changed.options = json!({"privateToken": "beta"});
        assert_eq!(
            public_redacted_profile_structure(&profile),
            public_redacted_profile_structure(&option_secret_changed),
            "arbitrary option values must not enter the public revision"
        );
        let mut option_shape_changed = profile.clone();
        option_shape_changed.options = json!({"privateToken": "alpha", "mode": "review"});
        assert_ne!(
            public_redacted_profile_structure(&profile),
            public_redacted_profile_structure(&option_shape_changed),
            "public option keys must revise Thread Context"
        );
    }
}
