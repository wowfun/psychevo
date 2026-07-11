use super::*;
use psychevo_runtime_host::{
    ControlState as HostControlState, ReadinessStatus as HostReadinessStatus,
    RuntimeAccountRateLimits as HostRuntimeAccountRateLimits,
    RuntimeCapability as HostRuntimeCapability,
    RuntimeControlDescriptor as HostRuntimeControlDescriptor,
    RuntimeCreditsSnapshot as HostRuntimeCreditsSnapshot, RuntimeGoal as HostRuntimeGoal,
    RuntimeGoalStatus as HostRuntimeGoalStatus,
    RuntimeRateLimitReachedType as HostRuntimeRateLimitReachedType,
    RuntimeRateLimitSnapshot as HostRuntimeRateLimitSnapshot,
    RuntimeRateLimitWindow as HostRuntimeRateLimitWindow, RuntimeSnapshot as HostRuntimeSnapshot,
    RuntimeSpendControlLimitSnapshot as HostRuntimeSpendControlLimitSnapshot,
    RuntimeStability as HostRuntimeStability, SnapshotMode as HostSnapshotMode,
    SnapshotQuery as HostSnapshotQuery, SnapshotScope as HostSnapshotScope,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostGoalResult {
    goal: Option<HostRuntimeGoal>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostGoalClearResult {
    cleared: bool,
}

#[derive(Debug, Clone)]
struct BoundCodexProfileTarget {
    thread_id: String,
    runtime_ref: String,
    native_session_id: Option<String>,
    cwd: PathBuf,
    binding_revision: u64,
}

const DIRECT_RUNTIME_MILESTONE_GATE: &str = "stable direct runtime milestone";

#[derive(Clone)]
struct RuntimeProfileRecord {
    config: RuntimeProfileConfig,
    generated: bool,
}

pub(super) fn runtime_profile_list_result(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<wire::RuntimeProfileListResult> {
    let profiles = runtime_profile_records(state, scope)?;
    Ok(wire::RuntimeProfileListResult {
        profiles: profiles
            .values()
            .map(|record| runtime_profile_view(state, scope, record, None))
            .collect::<psychevo_runtime::Result<Vec<_>>>()?,
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

pub(super) async fn runtime_snapshot_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSnapshotParams,
) -> psychevo_runtime::Result<wire::RuntimeSnapshotResult> {
    let profiles = runtime_profile_records(state, scope)?;
    let mut selected = Vec::new();
    for record in profiles.values() {
        if let Some(runtime_ref) = params.runtime_ref.as_deref()
            && record.config.id != runtime_ref
        {
            continue;
        }
        if runtime_profile_uses_host_snapshot(&record.config) && record.config.enabled {
            let query = runtime_profile_snapshot_query(scope, &record.config);
            state
                .inner
                .gateway
                .refresh_runtime_catalog_snapshot(query)
                .await?;
        }
        selected.push(runtime_profile_view(state, scope, record, None)?);
    }
    Ok(wire::RuntimeSnapshotResult {
        agents: runtime_snapshot_agents(state, scope, &profiles, &selected),
        profiles: selected,
    })
}

pub(super) fn runtime_profile_options(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<Vec<wire::RuntimeConfigOptionView>>> {
    let profiles = runtime_profile_records(state, scope)?;
    let Some(record) = profiles.get(runtime_ref) else {
        return Ok(None);
    };
    let profile = runtime_profile_view(state, scope, record, None)?;
    if let Some(snapshot) = runtime_profile_cached_snapshot(state, scope, &record.config) {
        return Ok(Some(
            snapshot
                .controls
                .iter()
                .map(host_runtime_config_option)
                .collect(),
        ));
    }
    let options = match profile.runtime.as_str() {
        "native" => vec![native_runtime_mode_option()],
        "codex" | "opencode" => Vec::new(),
        "acp" => return Ok(None),
        _ => Vec::new(),
    };
    Ok(Some(options))
}

pub(super) fn runtime_context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeContextReadParams,
) -> psychevo_runtime::Result<wire::RuntimeContextReadResult> {
    let requested_runtime_ref = params
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|runtime_ref| !runtime_ref.is_empty())
        .map(str::to_string);
    let thread_id = match params.thread_id {
        Some(thread_id) => Some(thread_id),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let binding = thread_id
        .as_deref()
        .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten();
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
    let bound_profile_record = binding
        .as_ref()
        .map(bound_runtime_profile_record)
        .transpose()?;
    if let (Some(binding), Some(requested_runtime_ref)) =
        (binding.as_ref(), requested_runtime_ref.as_deref())
        && binding.runtime_ref.as_deref() != Some(requested_runtime_ref)
    {
        return Err(runtime_rpc_error(
            "immutable_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            format!(
                "Thread `{}` is bound to Runtime Profile `{}`; start a new thread to use `{requested_runtime_ref}`.",
                binding.thread_id,
                binding.runtime_ref.as_deref().unwrap_or("unresolved"),
            ),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let source_lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&scope.source.source_key().0)?;
    let profile_records = runtime_profile_records(state, scope)?;
    let mut profiles = profile_records
        .values()
        .map(|record| runtime_profile_view(state, scope, record, None))
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
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
    let runtime_ref = binding
        .as_ref()
        .and_then(|binding| binding.runtime_ref.clone())
        .or_else(|| requested_runtime_ref.clone())
        .or_else(|| {
            source_lane
                .as_ref()
                .and_then(|lane| lane.draft_runtime_ref.clone())
        })
        .unwrap_or_else(|| "native".to_string());
    if !profiles.iter().any(|profile| profile.id == runtime_ref) {
        return Err(runtime_rpc_error(
            "runtime_profile_not_found",
            "configuration",
            wire::RuntimeRetryClassView::UserAction,
            format!("Unknown Runtime Profile `{runtime_ref}`."),
            None,
        ));
    }
    let selection_state = if binding.is_some() {
        "bound"
    } else if requested_runtime_ref.is_some() {
        "prospective"
    } else if source_lane
        .as_ref()
        .and_then(|lane| lane.draft_runtime_ref.as_ref())
        .is_some()
    {
        "draft"
    } else {
        "default"
    }
    .to_string();
    let capability_revision = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .map(|profile| profile.capability_revision.clone())
        .unwrap_or_default();
    let selected_record = bound_profile_record
        .as_ref()
        .filter(|record| record.config.id == runtime_ref)
        .or_else(|| profile_records.get(&runtime_ref));
    let cached_snapshot = selected_record.and_then(|record| {
        if let Some(binding) = binding.as_ref() {
            runtime_profile_cached_session_snapshot(state, record, binding)
        } else {
            runtime_profile_cached_snapshot(state, scope, &record.config)
        }
    });
    let controls = if let Some(snapshot) = cached_snapshot {
        snapshot
            .controls
            .iter()
            .filter_map(|control| {
                let mutable = snapshot.capabilities.iter().any(|capability| {
                    capability.id == format!("control.{}.set", control.id)
                        && capability.enabled
                        && capability.stability == HostRuntimeStability::Stable
                });
                host_runtime_context_control(control, binding.is_some(), mutable)
            })
            .collect()
    } else {
        selected_record
            .filter(|record| record.config.runtime == RuntimeProfileKind::Native)
            .map(|_| vec![native_runtime_mode_option()])
            .unwrap_or_default()
            .into_iter()
            .map(|option| wire::RuntimeControlDescriptorView {
                id: option.id,
                label: option.name,
                state: if binding.is_some() {
                    wire::RuntimeControlStateView::ReadOnlyCurrent
                } else if runtime_ref == "native" {
                    wire::RuntimeControlStateView::Selectable
                } else {
                    wire::RuntimeControlStateView::RuntimeDefault
                },
                current_value: option.current_value.map(Value::String),
                choices: option
                    .values
                    .into_iter()
                    .map(|choice| wire::RuntimeControlChoiceView {
                        value: Value::String(choice.value),
                        label: choice.name,
                        description: choice.description,
                    })
                    .collect(),
                depends_on: None,
                channel_safe: runtime_ref == "native",
                capability_revision: capability_revision.clone(),
            })
            .collect()
    };
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
                    wire::RuntimeSessionOwnershipView::ReadWrite
                }
                GatewayRuntimeBindingOwnership::ReadOnly => {
                    wire::RuntimeSessionOwnershipView::ReadOnly
                }
            },
            binding_revision: u64::try_from(binding.binding_revision).unwrap_or_default(),
        }
    });
    let active_session = binding
        .as_ref()
        .and_then(|binding| bound_runtime_session_view(state, binding));
    let children = binding
        .as_ref()
        .map(|binding| {
            state
                .inner
                .state
                .store()
                .gateway_runtime_child_bindings(&binding.thread_id)
        })
        .transpose()?
        .unwrap_or_default()
        .iter()
        .filter_map(|binding| bound_runtime_session_view(state, binding))
        .collect();
    let stability = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .and_then(|profile| profile.stability);
    let capabilities = profiles
        .iter()
        .find(|profile| profile.id == runtime_ref)
        .map(|profile| profile.capabilities.clone())
        .unwrap_or_default();
    let (goal, account_rate_limits) = thread_id
        .as_deref()
        .map(|thread_id| runtime_context_auxiliary_metadata(state, thread_id))
        .transpose()?
        .unwrap_or_default();
    Ok(wire::RuntimeContextReadResult {
        runtime_ref,
        selection_state,
        profiles,
        binding: binding_view,
        controls,
        stability,
        capabilities,
        active_session,
        children,
        goal,
        account_rate_limits,
    })
}

pub(super) async fn runtime_control_set_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeControlSetParams,
) -> psychevo_runtime::Result<wire::RuntimeControlSetResult> {
    let expected_capability_revision = parse_public_runtime_revision(
        "expectedCapabilityRevision",
        &params.expected_capability_revision,
    )?;
    let context = runtime_context_read_result(
        state,
        scope,
        wire::RuntimeContextReadParams {
            thread_id: None,
            runtime_ref: Some(params.runtime_ref.clone()),
            scope: None,
        },
    )?;
    let binding = context.binding.as_ref().ok_or_else(|| {
        runtime_rpc_error(
            "runtime_control_requires_binding",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            "Runtime control mutation requires a bound thread.".to_string(),
            None,
        )
    })?;
    if binding.binding_revision != params.expected_binding_revision {
        return Err(runtime_rpc_error(
            "stale_revision",
            "control",
            wire::RuntimeRetryClassView::SafeRetry,
            format!(
                "Runtime binding changed: expected {}, current {}.",
                params.expected_binding_revision, binding.binding_revision
            ),
            Some(format!("runtime-binding:{}", binding.thread_id)),
        ));
    }
    let control = context
        .controls
        .into_iter()
        .find(|control| control.id == params.control_id)
        .ok_or_else(|| {
            runtime_rpc_error(
                "unsupported",
                "control",
                wire::RuntimeRetryClassView::UserAction,
                format!("Runtime control is unavailable: {}", params.control_id),
                None,
            )
        })?;
    if control.capability_revision != params.expected_capability_revision {
        return Err(runtime_rpc_error(
            "stale_revision",
            "control",
            wire::RuntimeRetryClassView::SafeRetry,
            "Runtime capabilities changed; refresh Runtime Context.".to_string(),
            None,
        ));
    }
    if control.current_value.as_ref() == Some(&params.value) {
        return Ok(wire::RuntimeControlSetResult {
            changed: false,
            observed: true,
            control,
            binding_revision: binding.binding_revision,
        });
    }
    if control.state != wire::RuntimeControlStateView::Selectable {
        return Err(runtime_rpc_error(
            "unsupported",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            format!(
                "Runtime control `{}` is not an observed selectable session control; start a new thread with the requested value.",
                params.control_id
            ),
            None,
        ));
    }

    let mut options = state.run_options(scope.cwd.clone(), Some(binding.thread_id.clone()));
    options.runtime_ref = Some(params.runtime_ref.clone());
    let result = crate::execute_gateway_runtime_control(
        &state.inner.gateway,
        options,
        &params.runtime_ref,
        params.control_id.clone(),
        params.value.clone(),
        expected_capability_revision,
        params.expected_binding_revision,
    )
    .await?;
    if !result.observed
        || result.control.id != params.control_id
        || result.control.current_value.as_ref() != Some(&params.value)
    {
        return Err(runtime_rpc_error(
            "control_not_observed",
            "control",
            wire::RuntimeRetryClassView::SafeRetry,
            "The runtime did not observe the requested control value; no Gateway state was committed."
                .to_string(),
            None,
        ));
    }
    Ok(wire::RuntimeControlSetResult {
        changed: result.changed,
        observed: true,
        control: host_runtime_context_control(&result.control, true, true).ok_or_else(|| {
            runtime_rpc_error(
                "control_not_observed",
                "control",
                wire::RuntimeRetryClassView::SafeRetry,
                "The runtime returned no observed control value after mutation.".to_string(),
                None,
            )
        })?,
        binding_revision: binding.binding_revision,
    })
}

fn parse_public_runtime_revision(field: &str, value: &str) -> psychevo_runtime::Result<u64> {
    let canonical = value == "0"
        || (!value.starts_with('0')
            && !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit()));
    if !canonical {
        return Err(runtime_rpc_error(
            "invalid_revision",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            format!("`{field}` must be a canonical unsigned decimal string."),
            None,
        ));
    }
    value.parse::<u64>().map_err(|_| {
        runtime_rpc_error(
            "invalid_revision",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            format!("`{field}` is outside the supported unsigned 64-bit range."),
            None,
        )
    })
}

pub(super) async fn runtime_auth_action_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeAuthActionParams,
) -> psychevo_runtime::Result<wire::RuntimeAuthActionResult> {
    let input = params.input.as_ref().and_then(Value::as_object);
    let operation = match params.action.trim() {
        "repair" | "status" => {
            psychevo_runtime_host::RuntimeAuthOperation::Status { refresh: false }
        }
        "login" | "login_chatgpt" => psychevo_runtime_host::RuntimeAuthOperation::LoginChatgpt,
        "login_device_code" => psychevo_runtime_host::RuntimeAuthOperation::LoginDeviceCode,
        "cancel" => {
            let login_id = input
                .and_then(|input| input.get("loginId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|login_id| !login_id.is_empty())
                .ok_or_else(|| {
                    runtime_rpc_error(
                        "invalid_auth_input",
                        "authentication",
                        wire::RuntimeRetryClassView::UserAction,
                        "Cancelling runtime login requires input.loginId.".to_string(),
                        None,
                    )
                })?;
            psychevo_runtime_host::RuntimeAuthOperation::Cancel {
                login_id: login_id.to_string(),
            }
        }
        "logout" => psychevo_runtime_host::RuntimeAuthOperation::Logout,
        action => {
            return Err(runtime_rpc_error(
                "unsupported_auth_action",
                "authentication",
                wire::RuntimeRetryClassView::UserAction,
                format!("Unsupported runtime authentication action: {action}"),
                None,
            ));
        }
    };
    let result = crate::execute_gateway_runtime_auth(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), None),
        &params.runtime_ref,
        operation,
    )
    .await?;
    Ok(wire::RuntimeAuthActionResult {
        accepted: result.accepted,
        status: result.status,
        message: result.message,
        output: (!result.output.is_null()).then_some(result.output),
    })
}

pub(super) async fn runtime_goal_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeGoalReadParams,
) -> psychevo_runtime::Result<wire::RuntimeGoalReadResult> {
    let target = bound_codex_profile_target(state, scope, params.thread_id, false, true)?;
    let native_session_id = target
        .native_session_id
        .clone()
        .ok_or_else(missing_codex_goal_native_session)?;
    let result = crate::execute_gateway_codex_extension(
        &state.inner.gateway,
        state.run_options(target.cwd.clone(), Some(target.thread_id.clone())),
        &target.runtime_ref,
        Some(target.binding_revision),
        crate::GatewayCodexExtension::GoalRead {
            thread_id: target.thread_id.clone(),
            native_session_id,
            cwd: target.cwd,
        },
    )
    .await?;
    let result: HostGoalResult = decode_codex_extension_result("goal/read", result)?;
    let goal = result.goal.map(wire_runtime_goal);
    persist_runtime_context_value(state, &target.thread_id, "runtimeGoal", goal.as_ref())?;
    Ok(wire::RuntimeGoalReadResult {
        runtime_ref: target.runtime_ref,
        goal,
        binding_revision: target.binding_revision,
    })
}

pub(super) async fn runtime_goal_set_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeGoalSetParams,
) -> psychevo_runtime::Result<wire::RuntimeGoalSetResult> {
    let objective = params
        .objective
        .map(|objective| objective.trim().to_string());
    if objective.as_deref().is_some_and(str::is_empty) {
        return Err(invalid_runtime_goal_input(
            "Runtime goal objective must not be empty.",
        ));
    }
    if objective
        .as_deref()
        .is_some_and(|objective| objective.chars().count() > 4_000)
    {
        return Err(invalid_runtime_goal_input(
            "Runtime goal objective must be at most 4000 characters.",
        ));
    }
    if params.token_budget.is_some() && params.clear_token_budget {
        return Err(invalid_runtime_goal_input(
            "Runtime goal tokenBudget and clearTokenBudget cannot be supplied together.",
        ));
    }
    if params
        .token_budget
        .is_some_and(|token_budget| token_budget <= 0)
    {
        return Err(invalid_runtime_goal_input(
            "Runtime goal tokenBudget must be positive.",
        ));
    }
    if objective.is_none()
        && params.status.is_none()
        && params.token_budget.is_none()
        && !params.clear_token_budget
    {
        return Err(invalid_runtime_goal_input(
            "Runtime goal set requires objective, status, tokenBudget, or clearTokenBudget.",
        ));
    }

    let target = bound_codex_profile_target(state, scope, params.thread_id, true, true)?;
    let native_session_id = target
        .native_session_id
        .clone()
        .ok_or_else(missing_codex_goal_native_session)?;
    let token_budget = match (params.token_budget, params.clear_token_budget) {
        (Some(token_budget), false) => crate::GatewayGoalTokenBudgetUpdate::Set(token_budget),
        (None, true) => crate::GatewayGoalTokenBudgetUpdate::Clear,
        (None, false) => crate::GatewayGoalTokenBudgetUpdate::Unchanged,
        (Some(_), true) => unreachable!("validated token-budget input"),
    };
    let result = crate::execute_gateway_codex_extension(
        &state.inner.gateway,
        state.run_options(target.cwd.clone(), Some(target.thread_id.clone())),
        &target.runtime_ref,
        Some(target.binding_revision),
        crate::GatewayCodexExtension::GoalSet {
            thread_id: target.thread_id.clone(),
            native_session_id,
            cwd: target.cwd,
            objective,
            status: params.status.map(host_runtime_goal_status),
            token_budget,
        },
    )
    .await?;
    let result: HostGoalResult = decode_codex_extension_result("goal/set", result)?;
    let goal = result.goal.map(wire_runtime_goal).ok_or_else(|| {
        invalid_codex_extension_result("Codex goal/set returned no goal after mutation.")
    })?;
    persist_runtime_context_value(state, &target.thread_id, "runtimeGoal", Some(&goal))?;
    Ok(wire::RuntimeGoalSetResult {
        runtime_ref: target.runtime_ref,
        goal,
        binding_revision: target.binding_revision,
    })
}

pub(super) async fn runtime_goal_clear_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeGoalClearParams,
) -> psychevo_runtime::Result<wire::RuntimeGoalClearResult> {
    let target = bound_codex_profile_target(state, scope, params.thread_id, true, true)?;
    let native_session_id = target
        .native_session_id
        .clone()
        .ok_or_else(missing_codex_goal_native_session)?;
    let result = crate::execute_gateway_codex_extension(
        &state.inner.gateway,
        state.run_options(target.cwd.clone(), Some(target.thread_id.clone())),
        &target.runtime_ref,
        Some(target.binding_revision),
        crate::GatewayCodexExtension::GoalClear {
            thread_id: target.thread_id.clone(),
            native_session_id,
            cwd: target.cwd,
        },
    )
    .await?;
    let result: HostGoalClearResult = decode_codex_extension_result("goal/clear", result)?;
    if result.cleared {
        persist_runtime_context_value::<wire::RuntimeGoalView>(
            state,
            &target.thread_id,
            "runtimeGoal",
            None,
        )?;
    }
    Ok(wire::RuntimeGoalClearResult {
        runtime_ref: target.runtime_ref,
        cleared: result.cleared,
        binding_revision: target.binding_revision,
    })
}

pub(super) async fn runtime_account_rate_limits_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeAccountRateLimitsReadParams,
) -> psychevo_runtime::Result<wire::RuntimeAccountRateLimitsReadResult> {
    let requested_runtime_ref = params
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|runtime_ref| !runtime_ref.is_empty())
        .map(str::to_string);
    let inferred_thread_id = if params.thread_id.is_none() && requested_runtime_ref.is_none() {
        state.inner.gateway.resolve_source_thread(&scope.source)?
    } else {
        None
    };
    let thread_id = params.thread_id.or(inferred_thread_id);
    let (runtime_ref, cwd, binding_revision) = if let Some(thread_id) = thread_id.as_ref() {
        let target =
            bound_codex_profile_target(state, scope, Some(thread_id.clone()), false, false)?;
        if requested_runtime_ref
            .as_deref()
            .is_some_and(|requested| requested != target.runtime_ref)
        {
            return Err(runtime_rpc_error(
                "immutable_binding",
                "binding",
                wire::RuntimeRetryClassView::UserAction,
                format!(
                    "Thread `{}` is bound to Runtime Profile `{}`; account metadata must use that immutable Profile.",
                    target.thread_id, target.runtime_ref
                ),
                Some(format!("runtime-binding:{}", target.thread_id)),
            ));
        }
        (
            target.runtime_ref,
            target.cwd,
            Some(target.binding_revision),
        )
    } else {
        let runtime_ref = requested_runtime_ref.ok_or_else(|| {
            runtime_rpc_error(
                "runtime_ref_required",
                "configuration",
                wire::RuntimeRetryClassView::UserAction,
                "Account rate-limit read requires runtimeRef when no bound thread is selected."
                    .to_string(),
                None,
            )
        })?;
        (runtime_ref, scope.cwd.clone(), None)
    };
    let result = crate::execute_gateway_codex_extension(
        &state.inner.gateway,
        state.run_options(cwd.clone(), thread_id.clone()),
        &runtime_ref,
        binding_revision,
        crate::GatewayCodexExtension::AccountRateLimitsRead { cwd },
    )
    .await?;
    let rate_limits: HostRuntimeAccountRateLimits =
        decode_codex_extension_result("account/rateLimits/read", result)?;
    let account_rate_limits = wire_runtime_account_rate_limits(rate_limits);
    if let Some(thread_id) = thread_id.as_deref() {
        persist_runtime_context_value(
            state,
            thread_id,
            "runtimeAccountRateLimits",
            Some(&account_rate_limits),
        )?;
    }
    Ok(wire::RuntimeAccountRateLimitsReadResult {
        runtime_ref,
        account_rate_limits,
    })
}

fn bound_codex_profile_target(
    state: &WebState,
    scope: &ResolvedScope,
    requested_thread_id: Option<String>,
    require_write: bool,
    require_native_session: bool,
) -> psychevo_runtime::Result<BoundCodexProfileTarget> {
    let thread_id = match requested_thread_id {
        Some(thread_id) => Some(thread_id),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    }
    .ok_or_else(|| {
        runtime_rpc_error(
            "runtime_extension_requires_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "This Codex runtime operation requires a bound thread.".to_string(),
            None,
        )
    })?;
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)?
        .ok_or_else(|| {
            runtime_rpc_error(
                "runtime_extension_requires_binding",
                "binding",
                wire::RuntimeRetryClassView::UserAction,
                "This Codex runtime operation requires an immutable direct-runtime binding."
                    .to_string(),
                Some(format!("runtime-binding:{thread_id}")),
            )
        })?;
    if binding.status != GatewayRuntimeBindingStatus::Resolved {
        return Err(runtime_rpc_error(
            "unresolved_binding",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            binding
                .unresolved_reason
                .clone()
                .unwrap_or_else(|| "Select a Runtime Profile explicitly.".to_string()),
            Some(format!("runtime-binding:{thread_id}")),
        ));
    }
    let record = bound_runtime_profile_record(&binding)?;
    if record.config.runtime != RuntimeProfileKind::Codex {
        return Err(runtime_rpc_error(
            "codex_extension_unsupported",
            "configuration",
            wire::RuntimeRetryClassView::UserAction,
            "This bound Runtime Profile does not expose stable Codex goal or account metadata operations."
                .to_string(),
            Some(format!("runtime-binding:{thread_id}")),
        ));
    }
    if require_write && binding.ownership != GatewayRuntimeBindingOwnership::ReadWrite {
        return Err(runtime_rpc_error(
            "runtime_session_read_only",
            "control",
            wire::RuntimeRetryClassView::UserAction,
            "This runtime-native session is read-only; fork or resume an active session before changing its goal."
                .to_string(),
            Some(format!("runtime-binding:{thread_id}")),
        ));
    }
    if require_native_session && binding.native_session_id.is_none() {
        return Err(runtime_rpc_error(
            "runtime_native_session_missing",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "The runtime has not attached a native thread yet.".to_string(),
            Some(format!("runtime-binding:{thread_id}")),
        ));
    }
    Ok(BoundCodexProfileTarget {
        thread_id,
        runtime_ref: record.config.id,
        native_session_id: binding.native_session_id,
        cwd: PathBuf::from(binding.cwd),
        binding_revision: u64::try_from(binding.binding_revision).unwrap_or_default(),
    })
}

fn runtime_context_auxiliary_metadata(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<(
    Option<wire::RuntimeGoalView>,
    Option<wire::RuntimeAccountRateLimitsView>,
)> {
    let metadata = state.inner.state.store().session_metadata(thread_id)?;
    let goal = metadata
        .as_ref()
        .and_then(|metadata| metadata.get("runtimeGoal"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let account_rate_limits = metadata
        .as_ref()
        .and_then(|metadata| metadata.get("runtimeAccountRateLimits"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    Ok((goal, account_rate_limits))
}

fn persist_runtime_context_value<T: Serialize>(
    state: &WebState,
    thread_id: &str,
    key: &str,
    value: Option<&T>,
) -> psychevo_runtime::Result<()> {
    state.inner.state.store().set_session_metadata_field(
        thread_id,
        key,
        value.map(serde_json::to_value).transpose()?,
    )
}

fn decode_codex_extension_result<T: for<'de> Deserialize<'de>>(
    operation: &str,
    value: Value,
) -> psychevo_runtime::Result<T> {
    serde_json::from_value(value).map_err(|error| {
        invalid_codex_extension_result(&format!(
            "Codex {operation} result did not match the typed Gateway contract: {error}"
        ))
    })
}

fn invalid_runtime_goal_input(message: &str) -> Error {
    runtime_rpc_error(
        "invalid_runtime_goal_input",
        "control",
        wire::RuntimeRetryClassView::UserAction,
        message.to_string(),
        None,
    )
}

fn missing_codex_goal_native_session() -> Error {
    runtime_rpc_error(
        "runtime_native_session_missing",
        "binding",
        wire::RuntimeRetryClassView::UserAction,
        "The runtime has not attached a native thread yet.".to_string(),
        None,
    )
}

fn invalid_codex_extension_result(message: &str) -> Error {
    runtime_rpc_error(
        "runtime_extension_result_invalid",
        "transport",
        wire::RuntimeRetryClassView::Never,
        message.to_string(),
        None,
    )
}

fn host_runtime_goal_status(status: wire::RuntimeGoalStatusView) -> HostRuntimeGoalStatus {
    match status {
        wire::RuntimeGoalStatusView::Active => HostRuntimeGoalStatus::Active,
        wire::RuntimeGoalStatusView::Paused => HostRuntimeGoalStatus::Paused,
        wire::RuntimeGoalStatusView::Blocked => HostRuntimeGoalStatus::Blocked,
        wire::RuntimeGoalStatusView::UsageLimited => HostRuntimeGoalStatus::UsageLimited,
        wire::RuntimeGoalStatusView::BudgetLimited => HostRuntimeGoalStatus::BudgetLimited,
        wire::RuntimeGoalStatusView::Complete => HostRuntimeGoalStatus::Complete,
    }
}

fn wire_runtime_goal(goal: HostRuntimeGoal) -> wire::RuntimeGoalView {
    wire::RuntimeGoalView {
        objective: goal.objective,
        status: match goal.status {
            HostRuntimeGoalStatus::Active => wire::RuntimeGoalStatusView::Active,
            HostRuntimeGoalStatus::Paused => wire::RuntimeGoalStatusView::Paused,
            HostRuntimeGoalStatus::Blocked => wire::RuntimeGoalStatusView::Blocked,
            HostRuntimeGoalStatus::UsageLimited => wire::RuntimeGoalStatusView::UsageLimited,
            HostRuntimeGoalStatus::BudgetLimited => wire::RuntimeGoalStatusView::BudgetLimited,
            HostRuntimeGoalStatus::Complete => wire::RuntimeGoalStatusView::Complete,
        },
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        created_at: goal.created_at,
        updated_at: goal.updated_at,
    }
}

fn wire_runtime_account_rate_limits(
    limits: HostRuntimeAccountRateLimits,
) -> wire::RuntimeAccountRateLimitsView {
    wire::RuntimeAccountRateLimitsView {
        rate_limits: wire_runtime_rate_limit_snapshot(limits.rate_limits),
        rate_limits_by_limit_id: limits
            .rate_limits_by_limit_id
            .into_iter()
            .map(|(id, limits)| (id, wire_runtime_rate_limit_snapshot(limits)))
            .collect(),
        reset_credits_available: limits.reset_credits_available,
    }
}

fn wire_runtime_rate_limit_snapshot(
    limits: HostRuntimeRateLimitSnapshot,
) -> wire::RuntimeRateLimitSnapshotView {
    wire::RuntimeRateLimitSnapshotView {
        limit_id: limits.limit_id,
        limit_name: limits.limit_name,
        primary: limits.primary.map(wire_runtime_rate_limit_window),
        secondary: limits.secondary.map(wire_runtime_rate_limit_window),
        credits: limits.credits.map(wire_runtime_credits_snapshot),
        individual_limit: limits
            .individual_limit
            .map(wire_runtime_spend_control_limit_snapshot),
        plan_type: limits.plan_type,
        rate_limit_reached_type: limits.rate_limit_reached_type.map(|kind| match kind {
            HostRuntimeRateLimitReachedType::RateLimitReached => {
                wire::RuntimeRateLimitReachedTypeView::RateLimitReached
            }
            HostRuntimeRateLimitReachedType::WorkspaceOwnerCreditsDepleted => {
                wire::RuntimeRateLimitReachedTypeView::WorkspaceOwnerCreditsDepleted
            }
            HostRuntimeRateLimitReachedType::WorkspaceMemberCreditsDepleted => {
                wire::RuntimeRateLimitReachedTypeView::WorkspaceMemberCreditsDepleted
            }
            HostRuntimeRateLimitReachedType::WorkspaceOwnerUsageLimitReached => {
                wire::RuntimeRateLimitReachedTypeView::WorkspaceOwnerUsageLimitReached
            }
            HostRuntimeRateLimitReachedType::WorkspaceMemberUsageLimitReached => {
                wire::RuntimeRateLimitReachedTypeView::WorkspaceMemberUsageLimitReached
            }
        }),
    }
}

fn wire_runtime_rate_limit_window(
    window: HostRuntimeRateLimitWindow,
) -> wire::RuntimeRateLimitWindowView {
    wire::RuntimeRateLimitWindowView {
        used_percent: window.used_percent,
        window_duration_mins: window.window_duration_mins,
        resets_at: window.resets_at,
    }
}

fn wire_runtime_credits_snapshot(
    credits: HostRuntimeCreditsSnapshot,
) -> wire::RuntimeCreditsSnapshotView {
    wire::RuntimeCreditsSnapshotView {
        has_credits: credits.has_credits,
        unlimited: credits.unlimited,
        balance: credits.balance,
    }
}

fn wire_runtime_spend_control_limit_snapshot(
    limit: HostRuntimeSpendControlLimitSnapshot,
) -> wire::RuntimeSpendControlLimitSnapshotView {
    wire::RuntimeSpendControlLimitSnapshotView {
        limit: limit.limit,
        used: limit.used,
        remaining_percent: limit.remaining_percent,
        resets_at: limit.resets_at,
    }
}

pub(super) fn resolve_runtime_ref_peer_turn(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<crate::ResolvedPeerTurn>> {
    let runtime_ref = runtime_ref.trim();
    if runtime_ref.is_empty() || runtime_ref == "native" {
        return Ok(None);
    }
    let profiles = runtime_profile_records(state, scope)?;
    let Some(record) = profiles.get(runtime_ref) else {
        return Ok(None);
    };
    if record.config.runtime != RuntimeProfileKind::Acp {
        return Ok(None);
    }
    let backend_ref = record.config.backend_ref.as_deref().ok_or_else(|| {
        Error::Message(format!(
            "ACP runtime profile `{runtime_ref}` is missing backendRef"
        ))
    })?;
    let mut options = state.run_options(scope.cwd.clone(), None);
    options.runtime_ref = Some(backend_ref.to_string());
    crate::resolve_peer_turn(&options)
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
    let profiles = runtime_profile_records(state, scope)?;
    let Some(record) = profiles.get(runtime_ref) else {
        return Err(Error::Message(format!(
            "unknown runtime profile: {runtime_ref}"
        )));
    };
    if !record.config.enabled {
        return Err(Error::Message(format!(
            "runtime profile `{runtime_ref}` is disabled"
        )));
    }
    match record.config.runtime {
        RuntimeProfileKind::Native => Ok(()),
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode => Ok(()),
        RuntimeProfileKind::Acp => resolve_runtime_ref_peer_turn(state, scope, runtime_ref)?
            .map(|_| ())
            .ok_or_else(|| {
                Error::Message(format!(
                    "runtime profile `{runtime_ref}` references an unavailable ACP backend"
                ))
            }),
    }
}

pub(super) async fn runtime_health_check_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeHealthCheckParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles.get(&params.runtime_ref).ok_or_else(|| {
        Error::Message(format!("unknown runtime profile: {}", params.runtime_ref))
    })?;
    if runtime_profile_uses_host_snapshot(&record.config) && record.config.enabled {
        state
            .inner
            .gateway
            .refresh_runtime_snapshot(runtime_profile_snapshot_query(scope, &record.config))
            .await?;
    }
    Ok(serde_json::to_value(runtime_profile_view(
        state,
        scope,
        record,
        Some(gateway_now_ms()),
    )?)?)
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
    let existing = runtime_profile_records(state, scope)?
        .get(&params.id)
        .map(|record| record.config.clone());
    ensure_profile_config_for_runtime_profile_write(state, scope, params.target)?;
    // Read APIs expose env keys but never secret values. An editor round-trip with an empty env
    // therefore means "preserve", not "erase".
    let value = runtime_profile_config_json(&params, existing.as_ref())?;
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
        command: existing.config.command.clone(),
        args: existing.config.args.clone(),
        env: existing.config.env.clone(),
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

pub(super) fn runtime_session_list_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionListParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionListResult> {
    let runtime_ref = params.runtime_ref.unwrap_or_else(|| "native".to_string());
    if runtime_ref != "native" {
        return Ok(wire::RuntimeSessionListResult {
            runtime_ref: runtime_ref.clone(),
            supported: false,
            sessions: Vec::new(),
            next_cursor: None,
        });
    }
    let sessions = state
        .inner
        .state
        .store()
        .list_sessions_for_cwd_with_sources(&scope.cwd, &[])?
        .into_iter()
        .map(|summary| native_runtime_session_view(state, summary))
        .collect();
    Ok(wire::RuntimeSessionListResult {
        runtime_ref,
        supported: true,
        sessions,
        next_cursor: None,
    })
}

pub(super) async fn runtime_session_list_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionListParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionListResult> {
    let runtime_ref = params
        .runtime_ref
        .clone()
        .unwrap_or_else(|| "native".to_string());
    if runtime_ref == "native" {
        return runtime_session_list_result(state, scope, params);
    }
    let native_cursor = params
        .cursor
        .as_deref()
        .map(|cursor| {
            resolve_direct_runtime_session_list_cursor(state, scope, &runtime_ref, cursor)
        })
        .transpose()?;
    let (_, result) = crate::execute_gateway_runtime_session(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), None),
        &runtime_ref,
        psychevo_runtime_host::RuntimeSessionOperation::List,
        None,
        native_cursor,
        None,
    )
    .await?;
    let next_cursor = result
        .cursor
        .as_deref()
        .map(|cursor| cache_runtime_session_list_cursor(state, &runtime_ref, &scope.cwd, cursor));
    Ok(wire::RuntimeSessionListResult {
        runtime_ref: runtime_ref.clone(),
        supported: true,
        sessions: result
            .sessions
            .into_iter()
            .filter(|session| {
                session
                    .cwd
                    .as_deref()
                    .is_some_and(|cwd| runtime_session_cwd_matches_scope(Some(cwd), &scope.cwd))
            })
            .map(|session| host_runtime_session_view(state, scope, &runtime_ref, session))
            .collect(),
        next_cursor,
    })
}

pub(super) async fn runtime_session_read_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionReadParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref == "native" {
        if params.cursor.is_some() {
            return Err(runtime_rpc_error(
                "unsupported",
                "history",
                wire::RuntimeRetryClassView::UserAction,
                "Native Psychevo session history does not expose a pagination cursor.".to_string(),
                None,
            ));
        }
        return runtime_session_read_result(
            state,
            wire::RuntimeSessionParams {
                runtime_ref: params.runtime_ref,
                native_session_id: params.native_session_id,
                scope: params.scope,
            },
        );
    }
    let resolved = resolve_direct_runtime_session_handle(
        state,
        scope,
        &params.runtime_ref,
        &params.native_session_id,
    )
    .await?;
    let native_cursor = if let Some(cursor) = params.cursor.as_deref() {
        Some(resolve_direct_runtime_session_cursor(
            state,
            scope,
            &params.runtime_ref,
            &resolved,
            cursor,
        )?)
    } else {
        None
    };
    let native_page_cursor = native_cursor.clone();
    let page = read_validated_direct_runtime_session_page(
        state,
        scope,
        &params.runtime_ref,
        &resolved,
        native_page_cursor.clone(),
    )
    .await?;
    if let Some(thread_id) = resolved.thread_id.as_deref() {
        crate::import_runtime_session_history(
            &state.inner.state,
            thread_id,
            &params.runtime_ref,
            &page.session,
        )?;
    }
    let revisions = direct_runtime_session_revision_views(
        state,
        &params.runtime_ref,
        scope,
        &page.profile,
        &page.session,
        native_page_cursor,
    );
    let next_cursor = page.next_native_cursor.as_deref().map(|cursor| {
        cache_runtime_session_cursor(
            state,
            &params.runtime_ref,
            &scope.cwd,
            &page.session.native_session_id,
            cursor,
        )
    });
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: params.runtime_ref.clone(),
        native_session_id: params.native_session_id,
        supported: true,
        changed: false,
        session: Some(host_runtime_session_view(
            state,
            scope,
            &params.runtime_ref,
            page.session,
        )),
        message: page.message,
        revisions,
        next_cursor,
    })
}

struct ResolvedDirectRuntimeSession {
    native_session_id: String,
    thread_id: Option<String>,
}

struct ValidatedDirectRuntimeSessionPage {
    profile: RuntimeProfileConfig,
    session: psychevo_runtime_host::RuntimeSession,
    next_native_cursor: Option<String>,
    message: Option<String>,
}

async fn resolve_direct_runtime_session_handle(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    session_handle: &str,
) -> psychevo_runtime::Result<ResolvedDirectRuntimeSession> {
    for binding in state
        .inner
        .state
        .store()
        .gateway_runtime_bindings_for_runtime(runtime_ref)?
    {
        let Some(native_session_id) = binding.native_session_id.as_deref() else {
            continue;
        };
        let binding_cwd = Path::new(&binding.cwd);
        if !runtime_session_cwd_matches_scope(Some(binding_cwd), &scope.cwd) {
            continue;
        }
        if crate::runtime_session_handle(runtime_ref, binding_cwd, native_session_id)
            == session_handle
        {
            return Ok(ResolvedDirectRuntimeSession {
                native_session_id: native_session_id.to_string(),
                thread_id: Some(binding.thread_id),
            });
        }
    }
    if let Some(entry) = state
        .inner
        .runtime_session_handles
        .lock()
        .expect("runtime session handle cache")
        .get(session_handle)
        .cloned()
        && runtime_session_cache_entry_matches(
            &entry.runtime_ref,
            &entry.cwd,
            &entry.native_session_id,
            runtime_ref,
            &scope.cwd,
            None,
        )
    {
        return Ok(ResolvedDirectRuntimeSession {
            native_session_id: entry.native_session_id,
            thread_id: None,
        });
    }
    Err(runtime_rpc_error(
        "session_handle_not_found",
        "binding",
        wire::RuntimeRetryClassView::UserAction,
        "This Runtime Profile does not recognize the opaque session handle in this workspace; reload Native Sessions."
            .to_string(),
        None,
    ))
}

async fn read_validated_direct_runtime_session_page(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    resolved: &ResolvedDirectRuntimeSession,
    native_cursor: Option<String>,
) -> psychevo_runtime::Result<ValidatedDirectRuntimeSessionPage> {
    let (profile, mut result) = crate::execute_gateway_runtime_session(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), resolved.thread_id.clone()),
        runtime_ref,
        psychevo_runtime_host::RuntimeSessionOperation::Read,
        Some(resolved.native_session_id.clone()),
        native_cursor,
        None,
    )
    .await?;
    if result.sessions.len() != 1 {
        return Err(runtime_rpc_error(
            "runtime_session_read_mismatch",
            "history",
            wire::RuntimeRetryClassView::SafeRetry,
            "The runtime did not return exactly one validated session history page.".to_string(),
            None,
        ));
    }
    let session = result.sessions.pop().expect("one session was validated");
    if session.native_session_id != resolved.native_session_id {
        return Err(runtime_rpc_error(
            "runtime_session_read_mismatch",
            "history",
            wire::RuntimeRetryClassView::Never,
            "The runtime returned history for a different native session.".to_string(),
            None,
        ));
    }
    if !runtime_session_cwd_matches_scope(session.cwd.as_deref(), &scope.cwd) {
        return Err(runtime_rpc_error(
            "runtime_session_scope_mismatch",
            "history",
            wire::RuntimeRetryClassView::UserAction,
            "The runtime session history is not verified as belonging to this workspace."
                .to_string(),
            None,
        ));
    }
    let next_native_cursor = result.cursor.or_else(|| session.cursor.clone());
    Ok(ValidatedDirectRuntimeSessionPage {
        profile,
        session,
        next_native_cursor,
        message: result.message,
    })
}

fn resolve_direct_runtime_session_cursor(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    resolved: &ResolvedDirectRuntimeSession,
    public_cursor: &str,
) -> psychevo_runtime::Result<String> {
    if let Some(entry) = state
        .inner
        .runtime_session_cursors
        .lock()
        .expect("runtime session cursor cache")
        .get(public_cursor)
        .cloned()
        && runtime_session_cache_entry_matches(
            &entry.runtime_ref,
            &entry.cwd,
            &entry.native_session_id,
            runtime_ref,
            &scope.cwd,
            Some(&resolved.native_session_id),
        )
    {
        return Ok(entry.native_cursor);
    }
    Err(runtime_rpc_error(
        "session_cursor_not_found",
        "history",
        wire::RuntimeRetryClassView::UserAction,
        "This Runtime Profile does not recognize the opaque history cursor for this session; reload its history."
            .to_string(),
        None,
    ))
}

fn direct_runtime_session_revision_views(
    state: &WebState,
    runtime_ref: &str,
    scope: &ResolvedScope,
    profile: &RuntimeProfileConfig,
    session: &psychevo_runtime_host::RuntimeSession,
    native_page_cursor: Option<String>,
) -> Vec<wire::RuntimeSessionRevisionView> {
    if profile.runtime != RuntimeProfileKind::OpenCode
        || !session.actions.iter().any(|action| action == "revert")
    {
        return Vec::new();
    }
    let mut cache = state
        .inner
        .runtime_session_revisions
        .lock()
        .expect("runtime session revision cache");
    session
        .messages
        .iter()
        .filter_map(|message| {
            let native_message_id = direct_runtime_native_message_id(message)?;
            let revision_handle = runtime_session_revision_handle(
                runtime_ref,
                &scope.cwd,
                &session.native_session_id,
                native_message_id,
            );
            insert_bounded_runtime_session_cache(
                &mut cache,
                revision_handle.clone(),
                RuntimeSessionRevisionEntry {
                    runtime_ref: runtime_ref.to_string(),
                    cwd: runtime_session_cache_cwd(&scope.cwd),
                    native_session_id: session.native_session_id.clone(),
                    native_page_cursor: native_page_cursor.clone(),
                    native_message_id: native_message_id.to_string(),
                },
            );
            Some(wire::RuntimeSessionRevisionView {
                revision_handle,
                role: "user".to_string(),
                created_at_ms: message.created_at_ms,
            })
        })
        .collect()
}

fn direct_runtime_native_message_id(
    message: &psychevo_runtime_host::RuntimeHistoryMessage,
) -> Option<&str> {
    if message.role != "user" {
        return None;
    }
    message
        .metadata
        .as_ref()?
        .get("nativeMessageId")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn runtime_session_revision_handle(
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
    native_message_id: &str,
) -> String {
    use sha2::{Digest, Sha256};

    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "runtime-revision-v1\0{runtime_ref}\0{}\0{native_session_id}\0{native_message_id}",
            psychevo_runtime::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("rtr_{}", &format!("{digest:x}")[..24])
}

fn runtime_session_cursor_handle(
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
    native_cursor: &str,
) -> String {
    use sha2::{Digest, Sha256};

    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "runtime-cursor-v1\0{runtime_ref}\0{}\0{native_session_id}\0{native_cursor}",
            psychevo_runtime::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("rtc_{}", &format!("{digest:x}")[..24])
}

fn runtime_session_list_cursor_handle(
    runtime_ref: &str,
    cwd: &Path,
    native_cursor: &str,
) -> String {
    use sha2::{Digest, Sha256};

    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "runtime-list-cursor-v1\0{runtime_ref}\0{}\0{native_cursor}",
            psychevo_runtime::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("rtl_{}", &format!("{digest:x}")[..24])
}

const MAX_RUNTIME_SESSION_PUBLIC_HANDLE_ENTRIES: usize = 4096;

fn runtime_session_cache_cwd(cwd: &Path) -> String {
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    psychevo_runtime::normalized_native_path(&canonical)
        .display()
        .to_string()
}

fn runtime_session_cache_entry_matches(
    entry_runtime_ref: &str,
    entry_cwd: &str,
    entry_native_session_id: &str,
    runtime_ref: &str,
    cwd: &Path,
    expected_native_session_id: Option<&str>,
) -> bool {
    entry_runtime_ref == runtime_ref
        && entry_cwd == runtime_session_cache_cwd(cwd)
        && expected_native_session_id.is_none_or(|expected| entry_native_session_id == expected)
}

fn insert_bounded_runtime_session_cache<T>(cache: &mut HashMap<String, T>, key: String, value: T) {
    if !cache.contains_key(&key)
        && cache.len() >= MAX_RUNTIME_SESSION_PUBLIC_HANDLE_ENTRIES
        && let Some(expired) = cache.keys().next().cloned()
    {
        cache.remove(&expired);
    }
    cache.insert(key, value);
}

fn cache_runtime_session_cursor(
    state: &WebState,
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
    native_cursor: &str,
) -> String {
    let public_cursor =
        runtime_session_cursor_handle(runtime_ref, cwd, native_session_id, native_cursor);
    insert_bounded_runtime_session_cache(
        &mut state
            .inner
            .runtime_session_cursors
            .lock()
            .expect("runtime session cursor cache"),
        public_cursor.clone(),
        RuntimeSessionCursorEntry {
            runtime_ref: runtime_ref.to_string(),
            cwd: runtime_session_cache_cwd(cwd),
            native_session_id: native_session_id.to_string(),
            native_cursor: native_cursor.to_string(),
        },
    );
    public_cursor
}

fn cache_runtime_session_list_cursor(
    state: &WebState,
    runtime_ref: &str,
    cwd: &Path,
    native_cursor: &str,
) -> String {
    let public_cursor = runtime_session_list_cursor_handle(runtime_ref, cwd, native_cursor);
    insert_bounded_runtime_session_cache(
        &mut state
            .inner
            .runtime_session_list_cursors
            .lock()
            .expect("runtime session list cursor cache"),
        public_cursor.clone(),
        RuntimeSessionListCursorEntry {
            runtime_ref: runtime_ref.to_string(),
            cwd: runtime_session_cache_cwd(cwd),
            native_cursor: native_cursor.to_string(),
        },
    );
    public_cursor
}

fn resolve_direct_runtime_session_list_cursor(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    public_cursor: &str,
) -> psychevo_runtime::Result<String> {
    state
        .inner
        .runtime_session_list_cursors
        .lock()
        .expect("runtime session list cursor cache")
        .get(public_cursor)
        .cloned()
        .filter(|entry| {
            entry.runtime_ref == runtime_ref && entry.cwd == runtime_session_cache_cwd(&scope.cwd)
        })
        .map(|entry| entry.native_cursor)
        .ok_or_else(|| {
            runtime_rpc_error(
                "session_list_cursor_not_found",
                "history",
                wire::RuntimeRetryClassView::UserAction,
                "This Runtime Profile does not recognize the opaque Native Sessions cursor; reload Native Sessions."
                    .to_string(),
                None,
            )
        })
}

fn cache_runtime_session_handle(
    state: &WebState,
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
) -> String {
    let session_handle = crate::runtime_session_handle(runtime_ref, cwd, native_session_id);
    insert_bounded_runtime_session_cache(
        &mut state
            .inner
            .runtime_session_handles
            .lock()
            .expect("runtime session handle cache"),
        session_handle.clone(),
        RuntimeSessionHandleEntry {
            runtime_ref: runtime_ref.to_string(),
            cwd: runtime_session_cache_cwd(cwd),
            native_session_id: native_session_id.to_string(),
        },
    );
    session_handle
}

async fn resolve_direct_runtime_session_revision(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    resolved: &ResolvedDirectRuntimeSession,
    revision_handle: &str,
) -> psychevo_runtime::Result<String> {
    let entry = state
        .inner
        .runtime_session_revisions
        .lock()
        .expect("runtime session revision cache")
        .get(revision_handle)
        .cloned()
        .filter(|entry| {
            runtime_session_cache_entry_matches(
                &entry.runtime_ref,
                &entry.cwd,
                &entry.native_session_id,
                runtime_ref,
                &scope.cwd,
                Some(&resolved.native_session_id),
            )
        })
        .ok_or_else(|| {
            runtime_rpc_error(
                "revision_handle_not_found",
                "history",
                wire::RuntimeRetryClassView::UserAction,
                "This Runtime Profile does not recognize the opaque revision point for this session; reload its history."
                    .to_string(),
                None,
            )
        })?;
    let page = read_validated_direct_runtime_session_page(
        state,
        scope,
        runtime_ref,
        resolved,
        entry.native_page_cursor,
    )
    .await?;
    if page.profile.runtime != RuntimeProfileKind::OpenCode {
        return Err(runtime_rpc_error(
            "unsupported",
            "history",
            wire::RuntimeRetryClassView::Never,
            "This direct runtime has no stable native revert/unrevert operation.".to_string(),
            None,
        ));
    }
    if page.session.messages.iter().any(|message| {
        direct_runtime_native_message_id(message) == Some(entry.native_message_id.as_str())
            && runtime_session_revision_handle(
                runtime_ref,
                &scope.cwd,
                &resolved.native_session_id,
                &entry.native_message_id,
            ) == revision_handle
    }) {
        return Ok(entry.native_message_id);
    }
    Err(runtime_rpc_error(
        "stale_revision_handle",
        "history",
        wire::RuntimeRetryClassView::UserAction,
        "This opaque revision point is no longer present on the validated history page; reload session history."
            .to_string(),
        None,
    ))
}

pub(super) async fn runtime_session_attach_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref == "native" {
        return Err(runtime_rpc_error(
            "unsupported",
            "binding",
            wire::RuntimeRetryClassView::UserAction,
            "Native Psychevo sessions are already public threads and do not support runtime/session/attach."
                .to_string(),
            None,
        ));
    }
    let runtime_ref = params.runtime_ref;
    let session_handle = params.native_session_id;
    let resolved =
        resolve_direct_runtime_session_handle(state, scope, &runtime_ref, &session_handle).await?;
    let page =
        read_validated_direct_runtime_session_page(state, scope, &runtime_ref, &resolved, None)
            .await?;
    if page.session.parent_native_session_id.is_some() {
        return Ok(runtime_session_action_rejected(
            state,
            scope,
            &runtime_ref,
            session_handle,
            false,
            Some(page.session),
            "Runtime-native children already use their parent-owned read-only binding.".to_string(),
        ));
    }
    if page.session.ownership != psychevo_runtime_host::SessionOwnership::Active {
        return Ok(runtime_session_action_rejected(
            state,
            scope,
            &runtime_ref,
            session_handle,
            false,
            Some(page.session),
            "Attach read-only is available only while the native root session is active; use Resume for an idle transferable root."
                .to_string(),
        ));
    }
    if !page.session.actions.iter().any(|action| action == "read") {
        return Ok(runtime_session_action_rejected(
            state,
            scope,
            &runtime_ref,
            session_handle,
            false,
            Some(page.session),
            "This active native session does not expose readable history and cannot be attached."
                .to_string(),
        ));
    }

    let existing = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session(&runtime_ref, &resolved.native_session_id)?;
    let (thread_id, changed) = if let Some(binding) = existing {
        if binding.ownership != GatewayRuntimeBindingOwnership::ReadOnly
            || binding.parent_thread_id.is_some()
            || !runtime_session_cwd_matches_scope(Some(Path::new(&binding.cwd)), &scope.cwd)
        {
            return Err(runtime_rpc_error(
                "immutable_binding",
                "binding",
                wire::RuntimeRetryClassView::UserAction,
                "This active native session already has a different immutable public binding."
                    .to_string(),
                Some(format!("runtime-binding:{}", binding.thread_id)),
            ));
        }
        (binding.thread_id, false)
    } else {
        let thread_id = state.inner.state.store().create_session_with_metadata(
            &scope.cwd,
            "runtime_attach",
            "pending",
            &runtime_ref,
            Some(json!({
                "runtimeRef": runtime_ref,
                "runtimeDedupKey": crate::runtime_public_dedup_key(
                    &runtime_ref,
                    &page.session.native_dedup_key,
                ),
                "ownership": "read_only",
            })),
        )?;
        let create_result = (|| -> psychevo_runtime::Result<()> {
            if let Some(title) = public_runtime_session_title(&page.session) {
                state
                    .inner
                    .state
                    .store()
                    .set_session_title(&thread_id, &title)?;
            }
            crate::import_runtime_session_history(
                &state.inner.state,
                &thread_id,
                &runtime_ref,
                &page.session,
            )?;
            let fingerprint = runtime_profile_fingerprint(&page.profile);
            let revision = crate::runtime_profile_config_revision(&fingerprint);
            let profile_config_json = serde_json::to_string(&page.profile)?;
            state.inner.state.store().create_gateway_runtime_binding(
                psychevo_runtime::GatewayRuntimeBindingInput {
                    thread_id: &thread_id,
                    runtime_ref: &runtime_ref,
                    backend_kind: "runtime",
                    native_kind: page.profile.runtime.as_str(),
                    native_session_id: Some(&resolved.native_session_id),
                    cwd: &scope.cwd.display().to_string(),
                    profile_fingerprint: &fingerprint,
                    profile_revision: &revision.to_string(),
                    profile_config_json: &profile_config_json,
                    adapter_kind: page.profile.runtime.as_str(),
                    adapter_revision: env!("CARGO_PKG_VERSION"),
                    ownership: GatewayRuntimeBindingOwnership::ReadOnly,
                    parent_thread_id: None,
                },
            )?;
            Ok(())
        })();
        if let Err(error) = create_result {
            let _ = state.inner.state.delete_session(&thread_id);
            return Err(error);
        }
        (thread_id, true)
    };
    if !changed {
        crate::import_runtime_session_history(
            &state.inner.state,
            &thread_id,
            &runtime_ref,
            &page.session,
        )?;
    }
    state.inner.gateway.bind_source_thread(
        &scope.source,
        &thread_id,
        &GatewayBackendInfo {
            kind: BackendKind::Runtime,
            runtime_ref: Some(runtime_ref.clone()),
            native_id: Some(session_handle.clone()),
        },
        None,
    )?;
    let mut session = page.session;
    session.thread_id = Some(thread_id);
    let mut view = host_runtime_session_view(state, scope, &runtime_ref, session);
    view.ownership = wire::RuntimeSessionOwnershipView::ReadOnly;
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref,
        native_session_id: session_handle,
        supported: true,
        changed,
        session: Some(view),
        message: Some(if changed {
            "Attached the active native session to a new read-only public thread.".to_string()
        } else {
            "Opened the existing read-only public thread for this active native session."
                .to_string()
        }),
        revisions: Vec::new(),
        next_cursor: None,
    })
}

pub(super) async fn runtime_session_resume_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref == "native" {
        return runtime_session_resume_result(state, scope, params);
    }
    let runtime_ref = params.runtime_ref;
    let session_handle = params.native_session_id;
    let resolved =
        resolve_direct_runtime_session_handle(state, scope, &runtime_ref, &session_handle).await?;
    if let Some(result) = authorize_direct_runtime_session_action(
        state,
        scope,
        &runtime_ref,
        &resolved.native_session_id,
        &session_handle,
        resolved.thread_id.as_deref(),
        psychevo_runtime_host::RuntimeSessionOperation::Resume,
    )
    .await?
    {
        return Ok(result);
    }
    let existing_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session(&runtime_ref, &resolved.native_session_id)?;
    let (_, mut result) = crate::execute_gateway_runtime_session(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), resolved.thread_id.clone()),
        &runtime_ref,
        psychevo_runtime_host::RuntimeSessionOperation::Resume,
        Some(resolved.native_session_id.clone()),
        None,
        None,
    )
    .await?;
    let Some(mut session) = result.sessions.pop() else {
        return Ok(wire::RuntimeSessionMutationResult {
            runtime_ref: runtime_ref.clone(),
            native_session_id: session_handle,
            supported: true,
            changed: false,
            session: None,
            message: result.message,
            revisions: Vec::new(),
            next_cursor: None,
        });
    };
    if session.ownership == psychevo_runtime_host::SessionOwnership::Active {
        return Ok(wire::RuntimeSessionMutationResult {
            runtime_ref: runtime_ref.clone(),
            native_session_id: session_handle,
            supported: true,
            changed: false,
            session: Some(host_runtime_session_view(state, scope, &runtime_ref, session)),
            message: Some(
                "This native session is active. Attach read-only or Fork it instead of taking over."
                    .to_string(),
            ),
            revisions: Vec::new(),
            next_cursor: None,
        });
    }
    let thread_id = if let Some(binding) = existing_binding {
        binding.thread_id
    } else {
        let thread_id = state.inner.state.store().create_session_with_metadata(
            &scope.cwd,
            "runtime_resume",
            "pending",
            &runtime_ref,
            None,
        )?;
        let mut options = state.run_options(scope.cwd.clone(), Some(thread_id.clone()));
        options.runtime_ref = Some(runtime_ref.clone());
        let (profile, revision, fingerprint) = crate::resolve_gateway_runtime_profile(&options)?;
        let binding = crate::ensure_gateway_runtime_binding(
            &state.inner.state,
            &thread_id,
            &profile,
            revision,
            &fingerprint,
        )?;
        state
            .inner
            .state
            .store()
            .attach_gateway_runtime_native_session(
                &thread_id,
                binding.binding_revision,
                &resolved.native_session_id,
            )?;
        thread_id
    };
    crate::import_runtime_session_history(&state.inner.state, &thread_id, &runtime_ref, &session)?;
    let backend = GatewayBackendInfo {
        kind: BackendKind::Runtime,
        runtime_ref: Some(runtime_ref.clone()),
        native_id: Some(session_handle.clone()),
    };
    state
        .inner
        .gateway
        .bind_source_thread(&scope.source, &thread_id, &backend, None)?;
    session.thread_id = Some(thread_id);
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: runtime_ref.clone(),
        native_session_id: session_handle,
        supported: true,
        changed: true,
        session: Some(host_runtime_session_view(
            state,
            scope,
            &runtime_ref,
            session,
        )),
        message: result.message,
        revisions: Vec::new(),
        next_cursor: None,
    })
}

pub(super) async fn runtime_session_direct_action_result(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: String,
    session_handle: String,
    operation: psychevo_runtime_host::RuntimeSessionOperation,
    argument: Option<Value>,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    let resolved =
        resolve_direct_runtime_session_handle(state, scope, &runtime_ref, &session_handle).await?;
    direct_runtime_session_mutation(
        state,
        scope,
        DirectRuntimeSessionMutation {
            runtime_ref,
            session_handle,
            native_session_id: Some(resolved.native_session_id),
            bound_thread_id: resolved.thread_id,
            operation,
            argument,
        },
    )
    .await
}

pub(super) async fn runtime_session_direct_revision_result(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: String,
    session_handle: String,
    operation: psychevo_runtime_host::RuntimeSessionOperation,
    revision_handle: Option<String>,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    let resolved =
        resolve_direct_runtime_session_handle(state, scope, &runtime_ref, &session_handle).await?;
    let mut options = state.run_options(scope.cwd.clone(), resolved.thread_id.clone());
    options.runtime_ref = Some(runtime_ref.clone());
    let (profile, _, _) = crate::resolve_gateway_runtime_profile(&options)?;
    if profile.runtime != RuntimeProfileKind::OpenCode {
        return Ok(runtime_session_action_rejected(
            state,
            scope,
            &runtime_ref,
            session_handle,
            false,
            None,
            "This direct runtime has no stable native revert/unrevert operation.".to_string(),
        ));
    }
    let argument = match operation {
        psychevo_runtime_host::RuntimeSessionOperation::Revert => {
            let revision_handle = revision_handle.as_deref().ok_or_else(|| {
                runtime_rpc_error(
                    "missing_revision_handle",
                    "history",
                    wire::RuntimeRetryClassView::UserAction,
                    "Select an opaque revision point before reverting this session.".to_string(),
                    None,
                )
            })?;
            let native_message_id = resolve_direct_runtime_session_revision(
                state,
                scope,
                &runtime_ref,
                &resolved,
                revision_handle,
            )
            .await?;
            Some(json!({"messageID": native_message_id}))
        }
        psychevo_runtime_host::RuntimeSessionOperation::Unrevert => {
            if revision_handle.is_some() {
                return Err(runtime_rpc_error(
                    "unexpected_revision_handle",
                    "history",
                    wire::RuntimeRetryClassView::UserAction,
                    "Unrevert does not accept a revision handle.".to_string(),
                    None,
                ));
            }
            None
        }
        _ => {
            return Err(runtime_rpc_error(
                "unsupported",
                "history",
                wire::RuntimeRetryClassView::Never,
                "This RPC accepts only staged revert or unrevert operations.".to_string(),
                None,
            ));
        }
    };
    direct_runtime_session_mutation(
        state,
        scope,
        DirectRuntimeSessionMutation {
            runtime_ref,
            session_handle,
            native_session_id: Some(resolved.native_session_id),
            bound_thread_id: resolved.thread_id,
            operation,
            argument,
        },
    )
    .await
}

struct DirectRuntimeSessionMutation {
    runtime_ref: String,
    session_handle: String,
    native_session_id: Option<String>,
    bound_thread_id: Option<String>,
    operation: psychevo_runtime_host::RuntimeSessionOperation,
    argument: Option<Value>,
}

async fn direct_runtime_session_mutation(
    state: &WebState,
    scope: &ResolvedScope,
    input: DirectRuntimeSessionMutation,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    let DirectRuntimeSessionMutation {
        runtime_ref,
        session_handle,
        native_session_id,
        bound_thread_id,
        operation,
        argument,
    } = input;
    if operation != psychevo_runtime_host::RuntimeSessionOperation::Read {
        let Some(native_session_id) = native_session_id.as_deref() else {
            return Ok(runtime_session_action_rejected(
                state,
                scope,
                &runtime_ref,
                session_handle,
                false,
                None,
                "A native session id is required for this action.".to_string(),
            ));
        };
        if let Some(result) = authorize_direct_runtime_session_action(
            state,
            scope,
            &runtime_ref,
            native_session_id,
            &session_handle,
            bound_thread_id.as_deref(),
            operation,
        )
        .await?
        {
            return Ok(result);
        }
    }
    let (_, result) = crate::execute_gateway_runtime_session(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), bound_thread_id),
        &runtime_ref,
        operation,
        native_session_id,
        None,
        argument,
    )
    .await?;
    let session = result
        .sessions
        .into_iter()
        .next()
        .map(|session| {
            if operation == psychevo_runtime_host::RuntimeSessionOperation::Read
                && let Ok(Some(binding)) = state
                    .inner
                    .state
                    .store()
                    .gateway_runtime_binding_by_native_session(
                        &runtime_ref,
                        &session.native_session_id,
                    )
            {
                crate::import_runtime_session_history(
                    &state.inner.state,
                    &binding.thread_id,
                    &runtime_ref,
                    &session,
                )?;
            }
            Ok::<wire::RuntimeSessionView, psychevo_runtime::Error>(host_runtime_session_view(
                state,
                scope,
                &runtime_ref,
                session,
            ))
        })
        .transpose()?;
    let returned_session_handle = session
        .as_ref()
        .map(|session| session.native_session_id.clone())
        .unwrap_or(session_handle);
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref,
        native_session_id: returned_session_handle,
        supported: true,
        changed: result.changed,
        session,
        message: result.message,
        revisions: Vec::new(),
        next_cursor: None,
    })
}

async fn authorize_direct_runtime_session_action(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    native_session_id: &str,
    session_handle: &str,
    bound_thread_id: Option<&str>,
    operation: psychevo_runtime_host::RuntimeSessionOperation,
) -> psychevo_runtime::Result<Option<wire::RuntimeSessionMutationResult>> {
    let (_, mut result) = crate::execute_gateway_runtime_session(
        &state.inner.gateway,
        state.run_options(scope.cwd.clone(), bound_thread_id.map(str::to_string)),
        runtime_ref,
        psychevo_runtime_host::RuntimeSessionOperation::Read,
        Some(native_session_id.to_string()),
        None,
        None,
    )
    .await?;
    let Some(session) = result.sessions.pop() else {
        return Ok(Some(runtime_session_action_rejected(
            state,
            scope,
            runtime_ref,
            session_handle.to_string(),
            true,
            None,
            "The runtime did not return the requested session; no action was applied.".to_string(),
        )));
    };
    if session.native_session_id != native_session_id {
        return Ok(Some(runtime_session_action_rejected(
            state,
            scope,
            runtime_ref,
            session_handle.to_string(),
            true,
            Some(session),
            "The runtime returned a different native session; no action was applied.".to_string(),
        )));
    }
    if !runtime_session_cwd_matches_scope(session.cwd.as_deref(), &scope.cwd) {
        return Ok(Some(runtime_session_action_rejected(
                state,
                scope,
                runtime_ref,
                session_handle.to_string(),
                true,
                Some(session),
                "This runtime session is not verified as belonging to the requested workspace; no action was applied."
                    .to_string(),
            )));
    }
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session(runtime_ref, native_session_id)?;
    let has_read_write_binding = binding
        .as_ref()
        .is_some_and(|binding| binding.ownership == GatewayRuntimeBindingOwnership::ReadWrite);
    if let Some(binding) = binding.as_ref() {
        if !runtime_session_cwd_matches_scope(Some(Path::new(&binding.cwd)), &scope.cwd) {
            return Ok(Some(runtime_session_action_rejected(
                    state,
                    scope,
                    runtime_ref,
                    session_handle.to_string(),
                    true,
                    Some(session),
                    "The persisted runtime binding belongs to another workspace; no action was applied."
                        .to_string(),
                )));
        }
        if !runtime_session_operation_allowed_for_binding(binding.ownership, operation) {
            return Ok(Some(runtime_session_action_rejected(
                    state,
                    scope,
                    runtime_ref,
                    session_handle.to_string(),
                    true,
                    Some(session),
                    "Runtime-native child sessions are read-only; open the parent or Fork this session."
                        .to_string(),
                )));
        }
    }
    if operation != psychevo_runtime_host::RuntimeSessionOperation::Fork {
        if session.parent_native_session_id.is_some() {
            return Ok(Some(runtime_session_action_rejected(
                state,
                scope,
                runtime_ref,
                session_handle.to_string(),
                true,
                Some(session),
                "Runtime-native child sessions are read-only; open the parent or Fork this session."
                    .to_string(),
            )));
        }
        match session.ownership {
            psychevo_runtime_host::SessionOwnership::ReadWrite => {}
            psychevo_runtime_host::SessionOwnership::Active => {
                return Ok(Some(runtime_session_action_rejected(
                        state,
                        scope,
                        runtime_ref,
                        session_handle.to_string(),
                        true,
                        Some(session),
                        "This native session is active. Fork it instead of taking it over or mutating it."
                            .to_string(),
                    )));
            }
            psychevo_runtime_host::SessionOwnership::ReadOnly => {
                if !has_read_write_binding
                    && operation != psychevo_runtime_host::RuntimeSessionOperation::Resume
                {
                    return Ok(Some(runtime_session_action_rejected(
                            state,
                            scope,
                            runtime_ref,
                            session_handle.to_string(),
                            true,
                            Some(session),
                            "This idle native session is not writable yet. Resume it or Fork it before mutating it."
                                .to_string(),
                        )));
                }
            }
        }
    }
    let action = runtime_session_operation_name(operation);
    if !session.actions.iter().any(|candidate| candidate == action) {
        return Ok(Some(runtime_session_action_rejected(
            state,
            scope,
            runtime_ref,
            session_handle.to_string(),
            false,
            Some(session),
            format!("This runtime session does not declare the `{action}` action."),
        )));
    }
    Ok(None)
}

fn runtime_session_action_rejected(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    session_handle: String,
    supported: bool,
    session: Option<psychevo_runtime_host::RuntimeSession>,
    message: String,
) -> wire::RuntimeSessionMutationResult {
    wire::RuntimeSessionMutationResult {
        runtime_ref: runtime_ref.to_string(),
        native_session_id: session_handle,
        supported,
        changed: false,
        session: session
            .map(|session| host_runtime_session_view(state, scope, runtime_ref, session)),
        message: Some(message),
        revisions: Vec::new(),
        next_cursor: None,
    }
}

fn runtime_session_cwd_matches_scope(session_cwd: Option<&Path>, scope_cwd: &Path) -> bool {
    let Some(session_cwd) = session_cwd.filter(|cwd| cwd.is_absolute()) else {
        return false;
    };
    let Ok(session_cwd) = session_cwd.canonicalize() else {
        return false;
    };
    let Ok(scope_cwd) = scope_cwd.canonicalize() else {
        return false;
    };
    psychevo_runtime::normalized_native_path(&session_cwd)
        == psychevo_runtime::normalized_native_path(&scope_cwd)
}

fn runtime_session_operation_name(
    operation: psychevo_runtime_host::RuntimeSessionOperation,
) -> &'static str {
    match operation {
        psychevo_runtime_host::RuntimeSessionOperation::List => "list",
        psychevo_runtime_host::RuntimeSessionOperation::Read => "read",
        psychevo_runtime_host::RuntimeSessionOperation::Resume => "resume",
        psychevo_runtime_host::RuntimeSessionOperation::Fork => "fork",
        psychevo_runtime_host::RuntimeSessionOperation::Archive => "archive",
        psychevo_runtime_host::RuntimeSessionOperation::Unarchive => "unarchive",
        psychevo_runtime_host::RuntimeSessionOperation::Rename => "rename",
        psychevo_runtime_host::RuntimeSessionOperation::Delete => "delete",
        psychevo_runtime_host::RuntimeSessionOperation::Revert => "revert",
        psychevo_runtime_host::RuntimeSessionOperation::Unrevert => "unrevert",
    }
}

fn runtime_session_operation_allowed_for_binding(
    ownership: GatewayRuntimeBindingOwnership,
    operation: psychevo_runtime_host::RuntimeSessionOperation,
) -> bool {
    ownership == GatewayRuntimeBindingOwnership::ReadWrite
        || matches!(
            operation,
            psychevo_runtime_host::RuntimeSessionOperation::Read
                | psychevo_runtime_host::RuntimeSessionOperation::Fork
        )
}

fn host_runtime_session_view(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
    session: psychevo_runtime_host::RuntimeSession,
) -> wire::RuntimeSessionView {
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding_by_native_session(runtime_ref, &session.native_session_id)
        .ok()
        .flatten();
    let parent_thread_id = binding
        .as_ref()
        .and_then(|binding| binding.parent_thread_id.clone())
        .or_else(|| {
            session
                .parent_native_session_id
                .as_deref()
                .and_then(|parent| {
                    state
                        .inner
                        .state
                        .store()
                        .gateway_runtime_binding_by_native_session(runtime_ref, parent)
                        .ok()
                        .flatten()
                        .map(|binding| binding.thread_id)
                })
        });
    let session_cwd = session.cwd.as_deref().unwrap_or(&scope.cwd);
    let session_handle =
        cache_runtime_session_handle(state, runtime_ref, session_cwd, &session.native_session_id);
    let dedup_key = crate::runtime_public_dedup_key(runtime_ref, &session.native_dedup_key);
    let title = public_runtime_session_title(&session);
    let status = binding
        .as_ref()
        .filter(|binding| binding.parent_thread_id.is_some())
        .and_then(|binding| public_runtime_child_status(state, &binding.thread_id));
    let mut actions = session.actions.clone();
    let attachable_active_root = session.ownership
        == psychevo_runtime_host::SessionOwnership::Active
        && session.parent_native_session_id.is_none()
        && actions.iter().any(|action| action == "read")
        && binding.as_ref().is_none_or(|binding| {
            binding.ownership == GatewayRuntimeBindingOwnership::ReadOnly
                && binding.parent_thread_id.is_none()
        });
    if attachable_active_root && !actions.iter().any(|action| action == "attach") {
        actions.push("attach".to_string());
    }
    wire::RuntimeSessionView {
        native_session_id: session_handle,
        thread_id: binding.as_ref().map(|binding| binding.thread_id.clone()),
        title,
        archived: session.archived,
        updated_at_ms: session.updated_at_ms,
        parent_thread_id,
        status,
        native_dedup_key: dedup_key,
        fidelity: match session.fidelity {
            psychevo_runtime_host::HistoryFidelity::Full => wire::RuntimeHistoryFidelityView::Full,
            psychevo_runtime_host::HistoryFidelity::Summary => {
                wire::RuntimeHistoryFidelityView::Summary
            }
            psychevo_runtime_host::HistoryFidelity::Partial => {
                wire::RuntimeHistoryFidelityView::Partial
            }
        },
        ownership: match session.ownership {
            psychevo_runtime_host::SessionOwnership::ReadWrite => {
                wire::RuntimeSessionOwnershipView::ReadWrite
            }
            psychevo_runtime_host::SessionOwnership::ReadOnly => {
                wire::RuntimeSessionOwnershipView::ReadOnly
            }
            psychevo_runtime_host::SessionOwnership::Active => {
                wire::RuntimeSessionOwnershipView::Active
            }
        },
        actions,
    }
}

fn public_runtime_session_title(session: &psychevo_runtime_host::RuntimeSession) -> Option<String> {
    let title = session.title.as_deref()?.trim();
    if title.is_empty()
        || title.contains(&session.native_session_id)
        || (!session.native_dedup_key.is_empty() && title.contains(&session.native_dedup_key))
        || session
            .parent_native_session_id
            .as_deref()
            .is_some_and(|parent| !parent.is_empty() && title.contains(parent))
    {
        None
    } else {
        Some(title.to_string())
    }
}

fn public_runtime_child_status(state: &WebState, thread_id: &str) -> Option<String> {
    let metadata = state
        .inner
        .state
        .store()
        .session_metadata(thread_id)
        .ok()
        .flatten()?;
    let status = metadata.get("runtimeStatus")?.as_str()?.trim();
    let mut chars = status.chars();
    if !chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        || !chars
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        || status.len() > 64
    {
        return None;
    }
    Some(status.to_string())
}

pub(super) fn runtime_session_read_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    let Some(summary) = state
        .inner
        .state
        .store()
        .session_summary(&params.native_session_id)?
    else {
        return Err(Error::Message(format!(
            "runtime session not found: {}",
            params.native_session_id
        )));
    };
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: params.runtime_ref,
        native_session_id: params.native_session_id,
        supported: true,
        changed: false,
        session: Some(native_runtime_session_view(state, summary)),
        message: None,
        revisions: Vec::new(),
        next_cursor: None,
    })
}

pub(super) fn runtime_session_resume_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    if state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(
            params.native_session_id.clone(),
        ))
        .running
    {
        let mut result = runtime_session_read_result(state, params)?;
        result.message = Some(
            "This session is active. Open it read-only instead of taking over its source lane."
                .to_string(),
        );
        return Ok(result);
    }
    let backend = GatewayBackendInfo {
        kind: BackendKind::Psychevo,
        runtime_ref: Some("native".to_string()),
        native_id: Some(params.native_session_id.clone()),
    };
    state.inner.gateway.bind_source_thread(
        &scope.source,
        &params.native_session_id,
        &backend,
        None,
    )?;
    runtime_session_read_result(state, params).map(|mut result| {
        result.changed = true;
        result
    })
}

pub(super) fn runtime_session_archive_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
    archived: bool,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    if archived {
        state
            .inner
            .state
            .store()
            .archive_session(&params.native_session_id)?;
    } else {
        state
            .inner
            .state
            .store()
            .restore_session(&params.native_session_id)?;
    }
    runtime_session_read_result(state, params).map(|mut result| {
        result.changed = true;
        result
    })
}

pub(super) fn runtime_session_delete_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    state
        .inner
        .state
        .delete_session(&params.native_session_id)?;
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: params.runtime_ref,
        native_session_id: params.native_session_id,
        supported: true,
        changed: true,
        session: None,
        message: None,
        revisions: Vec::new(),
        next_cursor: None,
    })
}

pub(super) fn runtime_session_rename_result(
    state: &WebState,
    params: wire::RuntimeSessionRenameParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    state
        .inner
        .state
        .store()
        .set_session_title(&params.native_session_id, &params.title)?;
    runtime_session_read_result(
        state,
        wire::RuntimeSessionParams {
            runtime_ref: params.runtime_ref,
            native_session_id: params.native_session_id,
            scope: params.scope,
        },
    )
    .map(|mut result| {
        result.changed = true;
        result
    })
}

fn runtime_profile_records(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<BTreeMap<String, RuntimeProfileRecord>> {
    let mut configured =
        load_runtime_profile_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    for config in configured.values_mut() {
        validate_native_runtime_profile_identity(&config.id, config.runtime.as_str())?;
        hydrate_acp_profile_from_backend(config, &backends);
    }
    let referenced_backends = configured
        .values()
        .filter_map(|config| config.backend_ref.clone())
        .collect::<BTreeSet<_>>();
    let mut records = generated_runtime_profiles()
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
                    command: backend.command.clone(),
                    args: backend.args.clone(),
                    env: backend.env.clone(),
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
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode => {
            if let Some(backend) = agent.backend.as_ref() {
                return Err(Error::Message(format!(
                    "team member `{}` cannot pair ACP Agent Definition `{}` (backend `{}`) with direct Runtime Profile `{}`",
                    member.id, agent.name, backend.name, profile.id
                )));
            }
            let required = crate::direct_required_contribution_labels(agent);
            if !required.is_empty() {
                return Err(Error::Message(format!(
                    "team member `{}` Agent Definition `{}` requires {} that direct Runtime Profile `{}` cannot faithfully inject",
                    member.id,
                    agent.name,
                    required.join(", "),
                    profile.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_team_runtime_options(
    state: &WebState,
    scope: &ResolvedScope,
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
                "team member `{member_id}` safety override `{key}` is not an exact selectable runtime control; configure it on Runtime Profile `{}`",
                profile.id
            )));
        }
    }

    let snapshot = runtime_profile_cached_snapshot(state, scope, profile);
    let choice_matches = |choice: &Value, value: &str| match choice {
        Value::String(choice) => choice == value,
        Value::Bool(choice) => value.parse::<bool>().is_ok_and(|value| value == *choice),
        Value::Number(choice) => value
            .parse::<serde_json::Number>()
            .is_ok_and(|value| value == *choice),
        Value::Null | Value::Array(_) | Value::Object(_) => false,
    };
    let validate_choice = |control_id: &str, value: &str| -> psychevo_runtime::Result<()> {
        let Some(control) = snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .controls
                .iter()
                .find(|control| control.id == control_id)
        }) else {
            return Ok(());
        };
        if !control.choices.is_empty()
            && !control
                .choices
                .iter()
                .any(|choice| choice_matches(&choice.value, value))
        {
            return Err(Error::Message(format!(
                "team member `{member_id}` runtime option `{control_id}={value}` is not present in Runtime Profile `{}` capability revision {}",
                profile.id,
                snapshot
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.capability_revision)
            )));
        }
        Ok(())
    };
    let require_selectable_choice = |control_id: &str,
                                     value: &str|
     -> psychevo_runtime::Result<()> {
        let control = snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .controls
                .iter()
                .find(|control| control.id == control_id)
        });
        if control.is_some_and(|control| {
            control.state == HostControlState::Selectable
                && control
                    .choices
                    .iter()
                    .any(|choice| choice_matches(&choice.value, value))
        }) {
            return Ok(());
        }
        Err(Error::Message(format!(
            "team member `{member_id}` Codex runtime option `{control_id}={value}` requires an exact selectable control choice in the currently cached Runtime Profile `{}` capability snapshot; refresh the Runtime Profile catalog before saving",
            profile.id,
        )))
    };

    match profile.runtime {
        RuntimeProfileKind::Native => {
            if !options.is_empty() {
                return Err(Error::Message(format!(
                    "team member `{member_id}` Native Runtime Profile options are not supported by the managed-child path"
                )));
            }
        }
        RuntimeProfileKind::Codex => {
            if options.contains_key("agent") {
                return Err(Error::Message(format!(
                    "team member `{member_id}` Codex Runtime Profile does not support a native agent override"
                )));
            }
            for key in options.keys() {
                if !matches!(
                    key.as_str(),
                    "model" | "mode" | "effort" | "personality" | "serviceTier"
                ) {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` Codex runtime option `{key}` is unsupported by the stable catalog-backed Team contract"
                    )));
                }
            }
            if let Some(mode) = options.get("mode") {
                if mode == "plan" {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` Codex plan mode requires GUI Advanced interaction exposure and is unavailable for managed Team children"
                    )));
                }
                if !matches!(mode.as_str(), "default" | "auto-review" | "full-access") {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` Codex mode `{mode}` is unsupported"
                    )));
                }
                if mode == "full-access" && profile.sandbox.as_deref() != Some("danger-full-access")
                {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` Codex full-access mode requires Runtime Profile `{}` sandbox danger-full-access",
                        profile.id
                    )));
                }
            }
            if let Some(model) = options.get("model") {
                require_selectable_choice("model", model)?;
            }
            let advanced_options = options
                .iter()
                .filter(|(key, _)| matches!(key.as_str(), "effort" | "personality" | "serviceTier"))
                .collect::<Vec<_>>();
            if !advanced_options.is_empty() {
                let control_model = snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.extension.as_ref())
                    .and_then(|extension| extension.get("codex"))
                    .and_then(|codex| codex.get("controlModel"))
                    .and_then(Value::as_str);
                let effective_model = options
                    .get("model")
                    .map(String::as_str)
                    .or(profile.default_model.as_deref())
                    .or(control_model);
                if control_model.is_none() || effective_model != control_model {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` Codex catalog-backed options require the cached controls for the same effective model; configure the model on Runtime Profile `{}` and refresh its catalog before saving",
                        profile.id
                    )));
                }
                for (key, value) in advanced_options {
                    require_selectable_choice(key, value)?;
                }
            }
        }
        RuntimeProfileKind::OpenCode => {
            if profile
                .sandbox
                .as_deref()
                .is_some_and(|value| !value.is_empty())
                || !profile.workspace_roots.is_empty()
                || profile
                    .approval_mode
                    .as_deref()
                    .is_some_and(|value| !value.is_empty() && value != "default")
            {
                return Err(Error::Message(format!(
                    "Runtime Profile `{}` declares a safety policy OpenCode cannot exactly enforce",
                    profile.id
                )));
            }
            for key in options.keys() {
                if !matches!(key.as_str(), "model" | "mode" | "agent") {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` OpenCode runtime option `{key}` is unsupported"
                    )));
                }
            }
            if let Some(model) = options.get("model") {
                let valid = model
                    .split_once('/')
                    .is_some_and(|(provider, model)| !provider.is_empty() && !model.is_empty());
                if !valid {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` OpenCode model `{model}` must use a provider/model id"
                    )));
                }
            }
            if let Some(agent) = options.get("agent").or_else(|| options.get("mode")) {
                let configured_default = profile
                    .default_agent
                    .as_deref()
                    .or(profile.default_mode.as_deref());
                let known_builtin = matches!(agent.as_str(), "build" | "plan");
                if configured_default != Some(agent.as_str()) && !known_builtin {
                    let has_agent_catalog = snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.controls.iter().any(|control| {
                            control.id == "agent"
                                && control
                                    .choices
                                    .iter()
                                    .any(|choice| choice.value.as_str() == Some(agent.as_str()))
                        })
                    });
                    if !has_agent_catalog {
                        return Err(Error::Message(format!(
                            "team member `{member_id}` OpenCode agent `{agent}` is not in the cached Runtime Profile catalog; refresh the catalog before saving"
                        )));
                    }
                }
                validate_choice("agent", agent)?;
            }
        }
        RuntimeProfileKind::Acp => {
            for key in options.keys() {
                if !matches!(key.as_str(), "model" | "mode" | "agent") {
                    return Err(Error::Message(format!(
                        "team member `{member_id}` ACP runtime option `{key}` is unsupported by the stable Team contract"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn hydrate_acp_profile_from_backend(
    config: &mut RuntimeProfileConfig,
    backends: &BTreeMap<String, AgentBackendConfig>,
) {
    if config.runtime != RuntimeProfileKind::Acp {
        return;
    }
    let Some(backend) = config
        .backend_ref
        .as_deref()
        .and_then(|backend_ref| backends.get(backend_ref))
    else {
        return;
    };
    if config.command.is_none() {
        config.command.clone_from(&backend.command);
    }
    if config.args.is_empty() {
        config.args.clone_from(&backend.args);
    }
    for (key, value) in &backend.env {
        config
            .env
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

pub(super) fn generated_runtime_profiles() -> Vec<RuntimeProfileConfig> {
    vec![
        RuntimeProfileConfig {
            id: "native".to_string(),
            runtime: RuntimeProfileKind::Native,
            enabled: true,
            label: "Native".to_string(),
            backend_ref: None,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
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
            runtime: RuntimeProfileKind::Codex,
            enabled: true,
            label: "Codex".to_string(),
            backend_ref: None,
            command: Some("codex".to_string()),
            args: vec!["app-server".to_string(), "--stdio".to_string()],
            env: BTreeMap::new(),
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
            runtime: RuntimeProfileKind::OpenCode,
            enabled: true,
            label: "OpenCode".to_string(),
            backend_ref: None,
            command: Some("opencode".to_string()),
            args: vec!["serve".to_string()],
            env: BTreeMap::new(),
            default_model: None,
            default_mode: Some("build".to_string()),
            default_agent: Some("build".to_string()),
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
    ]
}

fn runtime_profile_view(
    state: &WebState,
    scope: &ResolvedScope,
    record: &RuntimeProfileRecord,
    checked_at_ms: Option<i64>,
) -> psychevo_runtime::Result<wire::RuntimeProfileView> {
    let config = &record.config;
    let fingerprint = runtime_profile_fingerprint(config);
    let revision = crate::runtime_profile_config_revision(&fingerprint);
    let snapshot = runtime_profile_cached_snapshot(state, scope, config);
    let mut health = runtime_profile_health(config, snapshot.as_ref(), checked_at_ms);
    apply_direct_runtime_milestone_gate(state, scope, config, &mut health)?;
    Ok(wire::RuntimeProfileView {
        id: config.id.clone(),
        runtime: config.runtime.as_str().to_string(),
        enabled: config.enabled,
        label: config.label.clone(),
        generated: record.generated,
        configured: !record.generated,
        command: config.command.clone(),
        args: config.args.clone(),
        backend_ref: config.backend_ref.clone(),
        provenance: match config.runtime {
            RuntimeProfileKind::Native => "Native".to_string(),
            RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode => "Direct".to_string(),
            RuntimeProfileKind::Acp => "ACP".to_string(),
        },
        profile_revision: revision.to_string(),
        capability_revision: snapshot
            .as_ref()
            .map_or(revision, |snapshot| snapshot.capability_revision)
            .to_string(),
        stability: snapshot
            .as_ref()
            .map(|snapshot| wire_runtime_stability(snapshot.stability))
            .or_else(|| {
                matches!(config.runtime, RuntimeProfileKind::Native)
                    .then_some(wire::RuntimeStabilityView::Stable)
            }),
        capabilities: snapshot
            .as_ref()
            .map(|snapshot| wire_runtime_capabilities(&snapshot.capabilities))
            .unwrap_or_else(|| {
                if matches!(config.runtime, RuntimeProfileKind::Native) {
                    vec![wire::RuntimeCapabilityView {
                        id: "turn.start".to_string(),
                        enabled: true,
                        stability: wire::RuntimeStabilityView::Stable,
                    }]
                } else {
                    Vec::new()
                }
            }),
        default_model: config.default_model.clone(),
        default_mode: config.default_mode.clone(),
        default_agent: config.default_agent.clone(),
        approval_mode: config.approval_mode.clone(),
        sandbox: config.sandbox.clone(),
        workspace_roots: config.workspace_roots.clone(),
        env_keys: config.env.keys().cloned().collect(),
        option_keys: runtime_profile_option_keys(&config.options),
        source_targets: runtime_profile_source_targets(state, scope, &config.id)?,
        readiness_stages: runtime_readiness_stages(config, snapshot.as_ref(), &health),
        health,
        diagnostics: runtime_profile_diagnostics(config),
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

fn runtime_profile_uses_host_snapshot(config: &RuntimeProfileConfig) -> bool {
    matches!(
        config.runtime,
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode
    )
}

fn runtime_profile_snapshot_query(
    scope: &ResolvedScope,
    config: &RuntimeProfileConfig,
) -> HostSnapshotQuery {
    let fingerprint = runtime_profile_fingerprint(config);
    let revision = crate::runtime_profile_config_revision(&fingerprint);
    HostSnapshotQuery {
        profile: crate::gateway_runtime_profile(config.clone(), revision, fingerprint),
        scope: HostSnapshotScope::Workspace {
            cwd: scope.cwd.clone(),
        },
        mode: HostSnapshotMode::Cached,
    }
}

fn runtime_profile_cached_snapshot(
    state: &WebState,
    scope: &ResolvedScope,
    config: &RuntimeProfileConfig,
) -> Option<HostRuntimeSnapshot> {
    runtime_profile_uses_host_snapshot(config)
        .then(|| runtime_profile_snapshot_query(scope, config))
        .and_then(|query| state.inner.gateway.cached_runtime_snapshot(&query))
}

fn runtime_profile_cached_session_snapshot(
    state: &WebState,
    record: &RuntimeProfileRecord,
    binding: &GatewayRuntimeBindingRecord,
) -> Option<HostRuntimeSnapshot> {
    if !runtime_profile_uses_host_snapshot(&record.config) {
        return None;
    }
    let native_session_id = binding.native_session_id.clone()?;
    let fingerprint = runtime_profile_fingerprint(&record.config);
    let revision = crate::runtime_profile_config_revision(&fingerprint);
    let query = HostSnapshotQuery {
        profile: crate::gateway_runtime_profile(record.config.clone(), revision, fingerprint),
        scope: HostSnapshotScope::Session {
            cwd: PathBuf::from(&binding.cwd),
            thread_id: binding.thread_id.clone(),
            native_session_id: Some(native_session_id),
        },
        mode: HostSnapshotMode::Cached,
    };
    state.inner.gateway.cached_runtime_snapshot(&query)
}

fn runtime_readiness_stages(
    config: &RuntimeProfileConfig,
    snapshot: Option<&HostRuntimeSnapshot>,
    health: &wire::RuntimeHealthView,
) -> Vec<wire::RuntimeReadinessStageView> {
    if let Some(snapshot) = snapshot {
        let mut stages = snapshot
            .readiness
            .iter()
            .map(|stage| wire::RuntimeReadinessStageView {
                id: stage.id.clone(),
                status: wire_readiness_status(stage.status),
                summary: stage.summary.clone(),
                observed_at_ms: stage.observed_at_ms,
            })
            .collect::<Vec<_>>();
        if health.status == "unsupported"
            && health.summary.starts_with(DIRECT_RUNTIME_MILESTONE_GATE)
        {
            stages.push(wire::RuntimeReadinessStageView {
                id: "direct-milestone".to_string(),
                status: wire::RuntimeReadinessStatusView::Unsupported,
                summary: health.summary.clone(),
                observed_at_ms: health.checked_at_ms,
            });
        }
        return stages;
    }
    let status = match health.status.as_str() {
        "ready" => wire::RuntimeReadinessStatusView::Ready,
        "missing" => wire::RuntimeReadinessStatusView::Missing,
        "needs_auth" => wire::RuntimeReadinessStatusView::NeedsAuth,
        "unsupported" | "disabled" => wire::RuntimeReadinessStatusView::Unsupported,
        "error" => wire::RuntimeReadinessStatusView::Error,
        _ => wire::RuntimeReadinessStatusView::Unchecked,
    };
    vec![wire::RuntimeReadinessStageView {
        id: if matches!(config.runtime, RuntimeProfileKind::Native) {
            "runtime"
        } else {
            "configuration"
        }
        .to_string(),
        status,
        summary: health.summary.clone(),
        observed_at_ms: health.checked_at_ms,
    }]
}

fn apply_direct_runtime_milestone_gate(
    state: &WebState,
    scope: &ResolvedScope,
    config: &RuntimeProfileConfig,
    health: &mut wire::RuntimeHealthView,
) -> psychevo_runtime::Result<()> {
    if health.status != "ready" {
        return Ok(());
    }
    let counterpart_kind = match config.runtime {
        RuntimeProfileKind::Codex => RuntimeProfileKind::OpenCode,
        RuntimeProfileKind::OpenCode => RuntimeProfileKind::Codex,
        RuntimeProfileKind::Native | RuntimeProfileKind::Acp => return Ok(()),
    };
    let records = runtime_profile_records(state, scope)?;
    let counterpart_ready = records.values().any(|record| {
        record.config.runtime == counterpart_kind
            && runtime_profile_health(
                &record.config,
                runtime_profile_cached_snapshot(state, scope, &record.config).as_ref(),
                None,
            )
            .status
                == "ready"
    });
    if counterpart_ready {
        return Ok(());
    }
    apply_direct_runtime_milestone_health(config.runtime, false, health);
    Ok(())
}

pub(super) fn apply_direct_runtime_milestone_health(
    runtime: RuntimeProfileKind,
    counterpart_ready: bool,
    health: &mut wire::RuntimeHealthView,
) {
    if health.status != "ready" || counterpart_ready {
        return;
    }
    let counterpart = match runtime {
        RuntimeProfileKind::Codex => RuntimeProfileKind::OpenCode.as_str(),
        RuntimeProfileKind::OpenCode => RuntimeProfileKind::Codex.as_str(),
        RuntimeProfileKind::Native | RuntimeProfileKind::Acp => return,
    };
    health.status = "unsupported".to_string();
    health.summary = format!(
        "{DIRECT_RUNTIME_MILESTONE_GATE} is blocked until an enabled {counterpart} profile passes its cached readiness and complete Stable capability gate"
    );
}

fn wire_readiness_status(status: HostReadinessStatus) -> wire::RuntimeReadinessStatusView {
    match status {
        HostReadinessStatus::Unchecked => wire::RuntimeReadinessStatusView::Unchecked,
        HostReadinessStatus::Ready => wire::RuntimeReadinessStatusView::Ready,
        HostReadinessStatus::Missing => wire::RuntimeReadinessStatusView::Missing,
        HostReadinessStatus::NeedsAuth => wire::RuntimeReadinessStatusView::NeedsAuth,
        HostReadinessStatus::Unsupported => wire::RuntimeReadinessStatusView::Unsupported,
        HostReadinessStatus::Error => wire::RuntimeReadinessStatusView::Error,
    }
}

fn wire_runtime_stability(stability: HostRuntimeStability) -> wire::RuntimeStabilityView {
    match stability {
        HostRuntimeStability::Stable => wire::RuntimeStabilityView::Stable,
        HostRuntimeStability::Experimental => wire::RuntimeStabilityView::Experimental,
        HostRuntimeStability::Unavailable => wire::RuntimeStabilityView::Unavailable,
    }
}

fn wire_runtime_capabilities(
    capabilities: &[HostRuntimeCapability],
) -> Vec<wire::RuntimeCapabilityView> {
    capabilities
        .iter()
        .map(|capability| wire::RuntimeCapabilityView {
            id: capability.id.clone(),
            enabled: capability.enabled,
            stability: wire_runtime_stability(capability.stability),
        })
        .collect()
}

pub(super) fn runtime_profile_health(
    config: &RuntimeProfileConfig,
    snapshot: Option<&HostRuntimeSnapshot>,
    checked_at_ms: Option<i64>,
) -> wire::RuntimeHealthView {
    if !config.enabled {
        return wire::RuntimeHealthView {
            status: "disabled".to_string(),
            summary: "runtime profile disabled".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    if matches!(config.runtime, RuntimeProfileKind::Native) {
        return wire::RuntimeHealthView {
            status: "ready".to_string(),
            summary: "native runtime available".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    if config
        .command
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: "runtime command missing".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    let Some(snapshot) = snapshot else {
        return wire::RuntimeHealthView {
            status: "unchecked".to_string(),
            summary: "runtime readiness has not been checked".to_string(),
            command_path: None,
            checked_at_ms,
        };
    };
    let mut status = if snapshot
        .readiness
        .iter()
        .any(|stage| stage.status == HostReadinessStatus::Error)
    {
        "error"
    } else if snapshot
        .readiness
        .iter()
        .any(|stage| stage.status == HostReadinessStatus::NeedsAuth)
    {
        "needs_auth"
    } else if snapshot
        .readiness
        .iter()
        .any(|stage| stage.status == HostReadinessStatus::Missing)
    {
        "missing"
    } else if snapshot
        .readiness
        .iter()
        .any(|stage| stage.status == HostReadinessStatus::Unsupported)
    {
        "unsupported"
    } else if snapshot.readiness.is_empty()
        || snapshot
            .readiness
            .iter()
            .any(|stage| stage.status == HostReadinessStatus::Unchecked)
    {
        "unchecked"
    } else {
        "ready"
    };
    let mut gate_summary = None;
    if status == "ready" && snapshot.stability != HostRuntimeStability::Stable {
        status = "unsupported";
        gate_summary = Some(format!(
            "runtime adapter stability is {:?}; the stable default path requires Stable",
            snapshot.stability
        ));
    }
    let missing_capabilities = mandatory_runtime_capability_ids(config.runtime)
        .iter()
        .copied()
        .filter(|required| {
            !snapshot.capabilities.iter().any(|capability| {
                capability.id == *required
                    && capability.enabled
                    && capability.stability == HostRuntimeStability::Stable
            })
        })
        .collect::<Vec<_>>();
    if status == "ready" && !missing_capabilities.is_empty() {
        status = "unsupported";
        gate_summary = Some(format!(
            "runtime adapter did not prove stable enabled mandatory capabilities: {}",
            missing_capabilities.join(", ")
        ));
    }
    let summary_status = match status {
        "error" => HostReadinessStatus::Error,
        "needs_auth" => HostReadinessStatus::NeedsAuth,
        "missing" => HostReadinessStatus::Missing,
        "unsupported" => HostReadinessStatus::Unsupported,
        "unchecked" => HostReadinessStatus::Unchecked,
        _ => HostReadinessStatus::Ready,
    };
    let summary = gate_summary.unwrap_or_else(|| {
        snapshot
            .readiness
            .iter()
            .find(|stage| stage.status == summary_status)
            .or_else(|| snapshot.readiness.first())
            .map(|stage| stage.summary.clone())
            .unwrap_or_else(|| "runtime readiness has not been checked".to_string())
    });
    wire::RuntimeHealthView {
        status: status.to_string(),
        summary,
        command_path: None,
        checked_at_ms: checked_at_ms.or_else(|| {
            snapshot
                .readiness
                .iter()
                .filter_map(|stage| stage.observed_at_ms)
                .max()
        }),
    }
}

pub(super) fn mandatory_runtime_capability_ids(
    runtime: RuntimeProfileKind,
) -> &'static [&'static str] {
    match runtime {
        RuntimeProfileKind::Codex => &[
            "session.list",
            "session.read",
            "session.resume",
            "session.fork",
            "session.archive",
            "session.unarchive",
            "session.rename",
            "session.delete",
            "turn.start",
            "turn.steer",
            "turn.interrupt",
            "interaction.command",
            "interaction.file",
            "interaction.permission",
            "children.read_only",
            "thread.compact",
            "thread.goal.read",
            "thread.goal.set",
            "thread.goal.clear",
            "thread.usage",
            "account.rate_limits.read",
            "timeline.plan",
            "timeline.diff",
        ],
        RuntimeProfileKind::OpenCode => &[
            "session.persistence",
            "session.list",
            "session.read",
            "session.resume",
            "session.fork",
            "session.revert",
            "session.unrevert",
            "session.rename",
            "session.archive",
            "session.delete",
            "turn.start",
            "turn.interrupt",
            "interaction.permission",
            "interaction.question",
            "children.read_only",
            "history.partial",
            "timeline.todos",
            "timeline.diff",
        ],
        RuntimeProfileKind::Native | RuntimeProfileKind::Acp => &[],
    }
}

fn runtime_profile_diagnostics(config: &RuntimeProfileConfig) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !config.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "runtime profile is disabled".to_string(),
        });
    }
    if !matches!(config.runtime, RuntimeProfileKind::Native) && config.command.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_command".to_string(),
            message: "runtime command is required for execution".to_string(),
        });
    }
    if config.runtime == RuntimeProfileKind::Acp && config.backend_ref.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_backend_ref".to_string(),
            message: "ACP Runtime Profiles require a backendRef.".to_string(),
        });
    }
    diagnostics
}

fn runtime_profile_option_keys(options: &Value) -> Vec<String> {
    options
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn runtime_snapshot_agents(
    state: &WebState,
    scope: &ResolvedScope,
    records: &BTreeMap<String, RuntimeProfileRecord>,
    profiles: &[wire::RuntimeProfileView],
) -> Vec<wire::RuntimeSnapshotAgentView> {
    profiles
        .iter()
        .filter(|profile| profile.enabled)
        .flat_map(|profile| {
            let Some(record) = records.get(&profile.id) else {
                return Vec::new();
            };
            let Some(snapshot) = runtime_profile_cached_snapshot(state, scope, &record.config)
            else {
                return Vec::new();
            };
            snapshot
                .controls
                .iter()
                .filter(|control| control.id == "agent")
                .flat_map(|control| &control.choices)
                .filter_map(|choice| {
                    let native_name = choice.value.as_str()?;
                    Some(wire::RuntimeSnapshotAgentView {
                        name: format!("{}-{native_name}", profile.id),
                        label: format!("{} {}", profile.label, choice.label),
                        runtime_ref: profile.id.clone(),
                        native_id: None,
                        mode: profile.default_mode.clone(),
                    })
                })
                .collect()
        })
        .collect()
}

fn host_runtime_config_option(
    control: &HostRuntimeControlDescriptor,
) -> wire::RuntimeConfigOptionView {
    wire::RuntimeConfigOptionView {
        id: control.id.clone(),
        name: control.label.clone(),
        description: None,
        category: Some(control.id.clone()),
        option_type: "select".to_string(),
        current_value: control
            .current_value
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_string),
        values: control
            .choices
            .iter()
            .filter_map(|choice| {
                Some(wire::RuntimeConfigOptionValueView {
                    value: choice.value.as_str()?.to_string(),
                    name: choice.label.clone(),
                    description: choice.description.clone(),
                    group: None,
                })
            })
            .collect(),
    }
}

fn host_runtime_context_control(
    control: &HostRuntimeControlDescriptor,
    bound: bool,
    mutable: bool,
) -> Option<wire::RuntimeControlDescriptorView> {
    if bound && !mutable && control.current_value.is_none() {
        return None;
    }
    Some(wire::RuntimeControlDescriptorView {
        id: control.id.clone(),
        label: control.label.clone(),
        state: if bound && !mutable {
            wire::RuntimeControlStateView::ReadOnlyCurrent
        } else {
            match control.state {
                HostControlState::RuntimeDefault => wire::RuntimeControlStateView::RuntimeDefault,
                HostControlState::ReadOnlyCurrent => wire::RuntimeControlStateView::ReadOnlyCurrent,
                HostControlState::Selectable => wire::RuntimeControlStateView::Selectable,
            }
        },
        current_value: control.current_value.clone(),
        choices: control
            .choices
            .iter()
            .map(|choice| wire::RuntimeControlChoiceView {
                value: choice.value.clone(),
                label: choice.label.clone(),
                description: choice.description.clone(),
            })
            .collect(),
        depends_on: control.depends_on.as_ref().map(|dependency| {
            wire::RuntimeControlDependencyView {
                control_id: dependency.control_id.clone(),
                value: dependency.value.clone(),
            }
        }),
        channel_safe: control.channel_safe,
        capability_revision: control.capability_revision.to_string(),
    })
}

fn runtime_profile_config_json(
    params: &wire::RuntimeProfileWriteParams,
    existing: Option<&RuntimeProfileConfig>,
) -> psychevo_runtime::Result<Value> {
    validate_runtime_profile_kind(&params.runtime)?;
    let backend_ref = params
        .backend_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let runtime_kind = match params.runtime.trim() {
        "native" => RuntimeProfileKind::Native,
        "codex" => RuntimeProfileKind::Codex,
        "opencode" => RuntimeProfileKind::OpenCode,
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
    if let Some(command) = params
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("command".to_string(), json!(command));
    }
    object.insert("args".to_string(), json!(trimmed_string_list(&params.args)));
    let env = if params.env.is_empty() {
        existing
            .map(|profile| profile.env.clone())
            .unwrap_or_default()
    } else {
        params
            .env
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    None
                } else {
                    Some((key.to_string(), value.to_string()))
                }
            })
            .collect::<BTreeMap<_, _>>()
    };
    object.insert("env".to_string(), json!(env));
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
    if matches!(value.trim(), "native" | "codex" | "opencode" | "acp") {
        Ok(())
    } else {
        Err(Error::Message(format!(
            "runtime profile kind `{value}` must be native, codex, opencode, or acp"
        )))
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

fn native_runtime_session_view(
    state: &WebState,
    summary: SessionSummary,
) -> wire::RuntimeSessionView {
    let session_id = summary.id.clone();
    let active = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(session_id.clone()))
        .running;
    wire::RuntimeSessionView {
        native_session_id: session_id.clone(),
        thread_id: Some(session_id.clone()),
        title: summary.title,
        archived: summary.archived_at_ms.is_some(),
        updated_at_ms: Some(summary.updated_at_ms),
        parent_thread_id: summary.parent_session_id,
        status: None,
        native_dedup_key: session_id,
        fidelity: wire::RuntimeHistoryFidelityView::Full,
        ownership: if active {
            wire::RuntimeSessionOwnershipView::Active
        } else {
            wire::RuntimeSessionOwnershipView::ReadWrite
        },
        actions: if active {
            vec!["rename".to_string()]
        } else {
            vec![
                "resume".to_string(),
                "archive".to_string(),
                "unarchive".to_string(),
                "rename".to_string(),
                "delete".to_string(),
                "revert".to_string(),
                "unrevert".to_string(),
            ]
        },
    }
}

fn bound_runtime_session_view(
    state: &WebState,
    binding: &psychevo_runtime::GatewayRuntimeBindingRecord,
) -> Option<wire::RuntimeSessionView> {
    let summary = state
        .inner
        .state
        .store()
        .session_summary(&binding.thread_id)
        .ok()
        .flatten()?;
    if binding
        .runtime_ref
        .as_deref()
        .is_none_or(|runtime_ref| runtime_ref == "native")
    {
        return Some(native_runtime_session_view(state, summary));
    }
    let runtime_ref = binding.runtime_ref.as_deref()?;
    let native_session_id = binding.native_session_id.as_deref()?;
    let active = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(binding.thread_id.clone()))
        .running;
    let ownership = match binding.ownership {
        GatewayRuntimeBindingOwnership::ReadOnly => wire::RuntimeSessionOwnershipView::ReadOnly,
        GatewayRuntimeBindingOwnership::ReadWrite if active => {
            wire::RuntimeSessionOwnershipView::Active
        }
        GatewayRuntimeBindingOwnership::ReadWrite => wire::RuntimeSessionOwnershipView::ReadWrite,
    };
    Some(wire::RuntimeSessionView {
        native_session_id: crate::runtime_session_handle(
            runtime_ref,
            Path::new(&binding.cwd),
            native_session_id,
        ),
        thread_id: Some(binding.thread_id.clone()),
        title: summary.title,
        archived: summary.archived_at_ms.is_some(),
        updated_at_ms: Some(summary.updated_at_ms),
        parent_thread_id: binding.parent_thread_id.clone(),
        status: binding
            .parent_thread_id
            .as_ref()
            .and_then(|_| public_runtime_child_status(state, &binding.thread_id)),
        native_dedup_key: runtime_session_public_dedup_key(runtime_ref, &binding.thread_id),
        fidelity: wire::RuntimeHistoryFidelityView::Partial,
        ownership,
        // The context cache does not probe the adapter. Capability-gated actions come from
        // runtime/session/list and must never be invented here.
        actions: Vec::new(),
    })
}

fn runtime_session_public_dedup_key(runtime_ref: &str, thread_id: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(format!("runtime-session\0{runtime_ref}\0{thread_id}").as_bytes());
    format!("rt_{}", &format!("{digest:x}")[..20])
}

fn unsupported_runtime_session_mutation<T: Into<String>>(
    runtime_ref: T,
    native_session_id: T,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: runtime_ref.into(),
        native_session_id: native_session_id.into(),
        supported: false,
        changed: false,
        session: None,
        message: Some(
            "runtime session operation is not supported by this adapter slice".to_string(),
        ),
        revisions: Vec::new(),
        next_cursor: None,
    })
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
    use psychevo_runtime_host::RuntimeSessionOperation;

    fn selectable_control(current_value: Option<Value>) -> HostRuntimeControlDescriptor {
        HostRuntimeControlDescriptor {
            id: "agent".to_string(),
            label: "Agent".to_string(),
            state: HostControlState::Selectable,
            current_value,
            choices: Vec::new(),
            depends_on: None,
            channel_safe: false,
            capability_revision: 7,
        }
    }

    #[test]
    fn bound_control_projection_requires_stable_mutation_capability() {
        let prospective = host_runtime_context_control(&selectable_control(None), false, false)
            .expect("unbound controls remain selectable for turn creation");
        assert_eq!(prospective.state, wire::RuntimeControlStateView::Selectable);

        assert!(host_runtime_context_control(&selectable_control(None), true, false).is_none());

        let observed = host_runtime_context_control(
            &selectable_control(Some(Value::String("review".to_string()))),
            true,
            false,
        )
        .expect("an observed bound control remains visible");
        assert_eq!(
            observed.state,
            wire::RuntimeControlStateView::ReadOnlyCurrent
        );

        let mutable = host_runtime_context_control(
            &selectable_control(Some(Value::String("review".to_string()))),
            true,
            true,
        )
        .expect("a stable mutable bound control remains visible");
        assert_eq!(mutable.state, wire::RuntimeControlStateView::Selectable);
        assert_eq!(mutable.capability_revision, "7");
    }

    #[test]
    fn public_runtime_revisions_parse_without_javascript_number_coercion() {
        assert_eq!(
            parse_public_runtime_revision("expectedCapabilityRevision", "9007199254740993")
                .expect("above-JavaScript-safe revision"),
            9_007_199_254_740_993
        );
        assert_eq!(
            parse_public_runtime_revision("expectedCapabilityRevision", "18446744073709551615")
                .expect("u64 max revision"),
            u64::MAX
        );
        for invalid in ["", "01", "+1", "-1", "18446744073709551616"] {
            assert!(
                parse_public_runtime_revision("expectedCapabilityRevision", invalid).is_err(),
                "{invalid} must fail closed"
            );
        }
    }

    #[test]
    fn read_only_child_allows_read_and_fork_but_rejects_mutation() {
        for allowed in [RuntimeSessionOperation::Read, RuntimeSessionOperation::Fork] {
            assert!(runtime_session_operation_allowed_for_binding(
                GatewayRuntimeBindingOwnership::ReadOnly,
                allowed
            ));
        }
        for denied in [
            RuntimeSessionOperation::Resume,
            RuntimeSessionOperation::Archive,
            RuntimeSessionOperation::Unarchive,
            RuntimeSessionOperation::Rename,
            RuntimeSessionOperation::Delete,
            RuntimeSessionOperation::Revert,
            RuntimeSessionOperation::Unrevert,
        ] {
            assert!(!runtime_session_operation_allowed_for_binding(
                GatewayRuntimeBindingOwnership::ReadOnly,
                denied
            ));
        }
    }
}
