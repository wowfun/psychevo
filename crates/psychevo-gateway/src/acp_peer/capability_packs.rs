const CODEX_ACP_AGENT_NAME: &str = "@agentclientprotocol/codex-acp";
const CODEX_ACP_REVIEWED_VERSION: &str = "1.1.2";
const OPENCODE_ACP_AGENT_NAME: &str = "OpenCode";
const OPENCODE_ACP_REVIEWED_VERSION: &str = "1.17.18";
const CODEX_QUOTA_SCHEMA_VERSION: u32 = 1;
const CODEX_QUOTA_MAX_MODEL_ENTRIES: usize = 32;
const CODEX_QUOTA_MAX_MODEL_CHARS: usize = 256;
const CODEX_QUOTA_MAX_TOKEN_COUNT: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CodexQuotaTokenCountSource {
    total_tokens: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CodexQuotaModelUsageSource {
    model: String,
    token_count: CodexQuotaTokenCountSource,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CodexPromptQuotaSource {
    token_count: Option<CodexQuotaTokenCountSource>,
    model_usage: Vec<CodexQuotaModelUsageSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexQuotaTokenCountProjection {
    total_tokens: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexQuotaModelUsageProjection {
    model: String,
    token_count: CodexQuotaTokenCountProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexPromptQuotaProjection {
    schema_version: u32,
    token_count: Option<CodexQuotaTokenCountProjection>,
    model_usage: Vec<CodexQuotaModelUsageProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexPromptQuotaRejection {
    InvalidSchema,
    BoundsExceeded,
}

impl CodexPromptQuotaRejection {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSchema => "invalid_schema",
            Self::BoundsExceeded => "bounds_exceeded",
        }
    }
}

impl From<CodexQuotaTokenCountSource> for CodexQuotaTokenCountProjection {
    fn from(source: CodexQuotaTokenCountSource) -> Self {
        Self {
            total_tokens: source.total_tokens,
            input_tokens: source.input_tokens,
            cached_input_tokens: source.cached_input_tokens,
            output_tokens: source.output_tokens,
            reasoning_output_tokens: source.reasoning_output_tokens,
        }
    }
}

fn codex_quota_token_count_is_bounded(token_count: &CodexQuotaTokenCountSource) -> bool {
    [
        token_count.total_tokens,
        token_count.input_tokens,
        token_count.cached_input_tokens,
        token_count.output_tokens,
        token_count.reasoning_output_tokens,
    ]
    .into_iter()
    .all(|value| value <= CODEX_QUOTA_MAX_TOKEN_COUNT)
}

fn project_codex_prompt_quota(
    initialized: &InitializeResponse,
    meta: Option<&Map<String, Value>>,
) -> Result<Option<CodexPromptQuotaProjection>, CodexPromptQuotaRejection> {
    let Some(agent_info) = initialized.agent_info.as_ref() else {
        return Ok(None);
    };
    if initialized.protocol_version != ProtocolVersion::V1
        || agent_info.name != CODEX_ACP_AGENT_NAME
        || agent_info.version != CODEX_ACP_REVIEWED_VERSION
    {
        return Ok(None);
    }
    let Some(quota) = meta.and_then(|meta| meta.get("quota")) else {
        return Ok(None);
    };
    let quota_object = quota
        .as_object()
        .ok_or(CodexPromptQuotaRejection::InvalidSchema)?;
    if quota_object.len() != 2
        || !quota_object.contains_key("token_count")
        || !quota_object.contains_key("model_usage")
    {
        return Err(CodexPromptQuotaRejection::InvalidSchema);
    }
    let model_usage = quota_object["model_usage"]
        .as_array()
        .ok_or(CodexPromptQuotaRejection::InvalidSchema)?;
    if model_usage.len() > CODEX_QUOTA_MAX_MODEL_ENTRIES {
        return Err(CodexPromptQuotaRejection::BoundsExceeded);
    }
    for entry in model_usage {
        let Some(model) = entry.get("model").and_then(Value::as_str) else {
            return Err(CodexPromptQuotaRejection::InvalidSchema);
        };
        if model.trim().is_empty()
            || model.chars().count() > CODEX_QUOTA_MAX_MODEL_CHARS
            || model.chars().any(char::is_control)
        {
            return Err(CodexPromptQuotaRejection::BoundsExceeded);
        }
    }
    let source = serde_json::from_value::<CodexPromptQuotaSource>(quota.clone())
        .map_err(|_| CodexPromptQuotaRejection::InvalidSchema)?;
    if source
        .token_count
        .as_ref()
        .is_some_and(|token_count| !codex_quota_token_count_is_bounded(token_count))
        || source
            .model_usage
            .iter()
            .any(|entry| !codex_quota_token_count_is_bounded(&entry.token_count))
    {
        return Err(CodexPromptQuotaRejection::BoundsExceeded);
    }
    if source.token_count.is_none() && !source.model_usage.is_empty() {
        return Err(CodexPromptQuotaRejection::InvalidSchema);
    }
    let mut model_ids = std::collections::BTreeSet::new();
    if source
        .model_usage
        .iter()
        .any(|entry| !model_ids.insert(entry.model.as_str()))
    {
        return Err(CodexPromptQuotaRejection::InvalidSchema);
    }
    Ok(Some(CodexPromptQuotaProjection {
        schema_version: CODEX_QUOTA_SCHEMA_VERSION,
        token_count: source.token_count.map(Into::into),
        model_usage: source
            .model_usage
            .into_iter()
            .map(|entry| CodexQuotaModelUsageProjection {
                model: entry.model,
                token_count: entry.token_count.into(),
            })
            .collect(),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpCapabilityPackKind {
    Codex,
    OpenCode,
}

impl AcpCapabilityPackKind {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

/// Returns a reviewed pack only when initialize itself proves the exact source
/// identity, version, and standard-v1 capability shape. This is intentionally
/// narrower than recognizing a same-name future Adapter: product extensions
/// must not be sent on a name-only guess.
pub(crate) fn reviewed_initialize_capability_pack(
    initialized: &InitializeResponse,
) -> Option<AcpCapabilityPackKind> {
    if initialized.protocol_version != ProtocolVersion::V1 {
        return None;
    }
    let identity = initialized.agent_info.as_ref()?;
    let kind = match (identity.name.as_str(), identity.version.as_str()) {
        (CODEX_ACP_AGENT_NAME, CODEX_ACP_REVIEWED_VERSION) => AcpCapabilityPackKind::Codex,
        (OPENCODE_ACP_AGENT_NAME, OPENCODE_ACP_REVIEWED_VERSION) => AcpCapabilityPackKind::OpenCode,
        _ => return None,
    };
    let capabilities = &initialized.agent_capabilities;
    let session = &capabilities.session_capabilities;
    let common = capabilities.load_session
        && capabilities.prompt_capabilities.image
        && capabilities.prompt_capabilities.embedded_context
        && session.list.is_some()
        && session.resume.is_some()
        && session.close.is_some();
    let compatible = common
        && match kind {
            AcpCapabilityPackKind::Codex => true,
            AcpCapabilityPackKind::OpenCode => serde_json::to_value(session)
                .ok()
                .and_then(|value| value.get("fork").cloned())
                .is_some_and(|value| !value.is_null()),
        };
    compatible.then_some(kind)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpCapabilityPackFact {
    pub(crate) id: String,
    pub(crate) enabled: bool,
    pub(crate) unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpCapabilityPackProjection {
    pub(crate) kind: AcpCapabilityPackKind,
    pub(crate) active: bool,
    pub(crate) diagnostic: Option<String>,
    pub(crate) facts: Vec<AcpCapabilityPackFact>,
}

/// Activates only reviewed, bounded Adapter contracts. The match is based on
/// the exact ACP `agentInfo.name`, the exact reviewed stable version, and the standard-v1
/// capability shape emitted by the reviewed local sources. No raw `_meta`
/// value crosses this seam.
pub(crate) fn project_acp_capability_pack(
    snapshot: &AcpSessionSnapshot,
) -> Option<AcpCapabilityPackProjection> {
    let (name, version) = snapshot.agent_pack_identity()?;
    let kind = match name {
        CODEX_ACP_AGENT_NAME => AcpCapabilityPackKind::Codex,
        OPENCODE_ACP_AGENT_NAME => AcpCapabilityPackKind::OpenCode,
        _ => return None,
    };
    let compatible_version = match kind {
        // These exact values come from the local source snapshots. No source
        // evidence certifies an arbitrary future patch or qualified build.
        AcpCapabilityPackKind::Codex => version == CODEX_ACP_REVIEWED_VERSION,
        AcpCapabilityPackKind::OpenCode => version == OPENCODE_ACP_REVIEWED_VERSION,
    };
    let compatible_schema = reviewed_standard_v1_shape(kind, snapshot);
    if !compatible_version || !compatible_schema {
        let diagnostic = if !compatible_version {
            format!(
                "{} ACP capability pack does not support agent version `{version}`",
                kind.id()
            )
        } else {
            format!(
                "{} ACP capability pack rejected an incompatible standard-v1 capability shape",
                kind.id()
            )
        };
        return Some(AcpCapabilityPackProjection {
            kind,
            active: false,
            facts: vec![AcpCapabilityPackFact {
                id: format!("pack.{}", kind.id()),
                enabled: false,
                unavailable_reason: Some(diagnostic.clone()),
            }],
            diagnostic: Some(diagnostic),
        });
    }

    let mut facts = vec![AcpCapabilityPackFact {
        id: format!("pack.{}", kind.id()),
        enabled: true,
        unavailable_reason: None,
    }];
    match kind {
        AcpCapabilityPackKind::Codex => {
            let api_key_auth = snapshot
                .capabilities
                .auth_methods
                .iter()
                .any(|method| method.id == "api-key");
            facts.push(AcpCapabilityPackFact {
                id: "codex.auth.apiKey".to_string(),
                enabled: api_key_auth,
                unavailable_reason: (!api_key_auth).then(|| {
                    "Codex ACP did not advertise its reviewed `api-key` authentication method."
                        .to_string()
                }),
            });
            let chat_gpt_auth = snapshot
                .capabilities
                .auth_methods
                .iter()
                .any(|method| method.id == "chat-gpt");
            facts.push(AcpCapabilityPackFact {
                id: "codex.auth.chatGpt".to_string(),
                enabled: chat_gpt_auth,
                unavailable_reason: (!chat_gpt_auth).then(|| {
                    "Codex ACP did not advertise its reviewed `chat-gpt` authentication method."
                        .to_string()
                }),
            });
            let gateway_provider = snapshot.capabilities.providers
                && snapshot
                    .capabilities
                    .auth_methods
                    .iter()
                    .any(|method| method.id == "gateway");
            facts.push(AcpCapabilityPackFact {
                id: "codex.provider.gateway".to_string(),
                enabled: gateway_provider,
                unavailable_reason: (!gateway_provider).then(|| {
                    "Codex ACP did not negotiate both provider configuration and its reviewed `gateway` auth method."
                        .to_string()
                }),
            });
            facts.push(AcpCapabilityPackFact {
                id: "codex.auth.logout".to_string(),
                enabled: snapshot.capabilities.auth_logout,
                unavailable_reason: (!snapshot.capabilities.auth_logout)
                    .then(|| "Codex ACP did not negotiate logout support.".to_string()),
            });
            facts.push(AcpCapabilityPackFact {
                id: "codex.goal".to_string(),
                enabled: snapshot
                    .available_commands
                    .iter()
                    .any(|command| command.name == "goal"),
                unavailable_reason: (!snapshot
                    .available_commands
                    .iter()
                    .any(|command| command.name == "goal"))
                .then(|| {
                    "Codex goal is available only when the Agent advertises the reviewed `/goal` prompt command."
                        .to_string()
                }),
            });
            let fast_mode = snapshot
                .options
                .iter()
                .any(|option| option.id == "fast-mode");
            facts.push(AcpCapabilityPackFact {
                id: "codex.fastMode".to_string(),
                enabled: fast_mode,
                unavailable_reason: (!fast_mode).then(|| {
                    "Codex fast mode is available only through the reviewed `fast-mode` config option."
                        .to_string()
                }),
            });
        }
        AcpCapabilityPackKind::OpenCode => {
            let login = snapshot
                .capabilities
                .auth_methods
                .iter()
                .any(|method| method.id == "opencode-login");
            facts.push(AcpCapabilityPackFact {
                id: "opencode.auth.login".to_string(),
                enabled: login,
                unavailable_reason: (!login).then(|| {
                    "OpenCode ACP did not advertise its reviewed login method.".to_string()
                }),
            });
            facts.push(AcpCapabilityPackFact {
                id: "opencode.sessionFork".to_string(),
                enabled: snapshot.capabilities.session.fork,
                unavailable_reason: (!snapshot.capabilities.session.fork)
                    .then(|| "OpenCode ACP did not negotiate session fork support.".to_string()),
            });
        }
    }
    facts.extend(direct_only_unavailable_facts());
    Some(AcpCapabilityPackProjection {
        kind,
        active: true,
        diagnostic: None,
        facts,
    })
}

fn reviewed_standard_v1_shape(kind: AcpCapabilityPackKind, snapshot: &AcpSessionSnapshot) -> bool {
    let prompt = &snapshot.capabilities.prompt_input;
    let session = &snapshot.capabilities.session;
    let common = prompt.text
        && prompt.image
        && prompt.embedded_context
        && session.load
        && session.list
        && session.resume
        && session.close;
    common
        && match kind {
            AcpCapabilityPackKind::Codex => true,
            AcpCapabilityPackKind::OpenCode => session.fork,
        }
}

fn direct_only_unavailable_facts() -> Vec<AcpCapabilityPackFact> {
    [
        (
            "direct.steer",
            "ACP does not expose direct-runtime steering.",
        ),
        (
            "direct.children",
            "ACP does not expose direct child-session projection.",
        ),
        ("direct.todo", "ACP does not expose direct todo state."),
        ("direct.diff", "ACP does not expose direct diff state."),
        ("direct.revert", "ACP does not expose direct revert state."),
    ]
    .into_iter()
    .map(|(id, reason)| AcpCapabilityPackFact {
        id: id.to_string(),
        enabled: false,
        unavailable_reason: Some(reason.to_string()),
    })
    .collect()
}

#[cfg(test)]
mod capability_pack_tests {
    use super::*;

    const CODEX_INITIALIZE_V1_FIXTURE: &str =
        include_str!("../../tests/fixtures/acp_capability_packs/codex_initialize_v1.json");
    const OPENCODE_INITIALIZE_V1_FIXTURE: &str =
        include_str!("../../tests/fixtures/acp_capability_packs/opencode_initialize_v1.json");

    fn initialized_fixture(fixture: &str) -> InitializeResponse {
        serde_json::from_str(fixture).expect("source-derived ACP initialize fixture")
    }

    fn codex_initialized(version: &str) -> InitializeResponse {
        let mut initialized = initialized_fixture(CODEX_INITIALIZE_V1_FIXTURE);
        initialized
            .agent_info
            .as_mut()
            .expect("Codex fixture agentInfo")
            .version = version.to_string();
        initialized
    }

    fn pack_snapshot(
        fixture: &str,
        version_override: Option<&str>,
        fork_override: Option<bool>,
    ) -> AcpSessionSnapshot {
        let mut raw: Value =
            serde_json::from_str(fixture).expect("source-derived ACP initialize fixture JSON");
        if let Some(version) = version_override {
            raw["agentInfo"]["version"] = Value::String(version.to_string());
        }
        if let Some(fork) = fork_override {
            let session = raw
                .pointer_mut("/agentCapabilities/sessionCapabilities")
                .and_then(Value::as_object_mut)
                .expect("fixture sessionCapabilities");
            if fork {
                session.insert("fork".to_string(), json!({}));
            } else {
                session.remove("fork");
            }
        }
        let initialized: InitializeResponse =
            serde_json::from_value(raw).expect("source-derived ACP initialize fixture");
        let session = new_acp_resident_session(
            &initialized,
            AcpResidentSessionInput {
                native_session_id: "native-pack".to_string(),
                modes: None,
                config_options: Vec::new(),
                session_epoch: 1,
                loaded_from_agent: false,
                mcp_servers: Vec::new(),
                mcp_declaration_fingerprint: String::new(),
            },
        );
        acp_session_snapshot(&session, 1)
    }

    #[test]
    fn codex_pack_requires_exact_identity_version_and_reviewed_shape() {
        assert_eq!(
            reviewed_initialize_capability_pack(&initialized_fixture(CODEX_INITIALIZE_V1_FIXTURE)),
            Some(AcpCapabilityPackKind::Codex)
        );
        let snapshot = pack_snapshot(CODEX_INITIALIZE_V1_FIXTURE, None, None);
        assert!(
            snapshot
                .capabilities
                .auth_methods
                .iter()
                .any(|method| method.id == "gateway"),
            "source-derived fixture must retain reviewed gateway auth"
        );
        let active = project_acp_capability_pack(&snapshot).expect("Codex candidate");
        assert!(active.active);
        assert_eq!(active.kind, AcpCapabilityPackKind::Codex);
        for capability in [
            "codex.auth.apiKey",
            "codex.auth.chatGpt",
            "codex.auth.logout",
        ] {
            assert!(
                active
                    .facts
                    .iter()
                    .any(|fact| fact.id == capability && fact.enabled),
                "source-derived Codex initialize fixture must enable {capability}"
            );
        }
        assert!(active
            .facts
            .iter()
            .any(|fact| fact.id == "codex.provider.gateway" && fact.enabled));
        assert!(
            active
                .facts
                .iter()
                .any(|fact| fact.id == "direct.revert" && !fact.enabled)
        );

        let rejected = project_acp_capability_pack(&pack_snapshot(
            CODEX_INITIALIZE_V1_FIXTURE,
            Some("1.2.0"),
            None,
        ))
        .expect("Codex candidate");
        assert!(!rejected.active);
        assert!(
            rejected
                .diagnostic
                .as_deref()
                .is_some_and(|value| value.contains("1.2.0"))
        );

        for version in ["1.1.3", "1.1.2-next.1", "1.1.2+unreviewed"] {
            let rejected = project_acp_capability_pack(&pack_snapshot(
                CODEX_INITIALIZE_V1_FIXTURE,
                Some(version),
                None,
            ))
            .expect("Codex candidate");
            assert!(!rejected.active, "unreviewed Codex version {version}");
        }
    }

    #[test]
    fn opencode_pack_requires_reviewed_fork_capability() {
        assert_eq!(
            reviewed_initialize_capability_pack(&initialized_fixture(
                OPENCODE_INITIALIZE_V1_FIXTURE
            )),
            Some(AcpCapabilityPackKind::OpenCode)
        );
        let active =
            project_acp_capability_pack(&pack_snapshot(OPENCODE_INITIALIZE_V1_FIXTURE, None, None))
                .expect("OpenCode candidate");
        assert!(active.active);
        for capability in ["opencode.auth.login", "opencode.sessionFork"] {
            assert!(
                active
                    .facts
                    .iter()
                    .any(|fact| fact.id == capability && fact.enabled),
                "source-derived OpenCode initialize fixture must enable {capability}"
            );
        }

        let rejected = project_acp_capability_pack(&pack_snapshot(
            OPENCODE_INITIALIZE_V1_FIXTURE,
            None,
            Some(false),
        ))
        .expect("OpenCode candidate");
        assert!(!rejected.active);
        assert!(
            rejected
                .diagnostic
                .as_deref()
                .is_some_and(|value| value.contains("capability shape"))
        );

        for version in ["1.17.19", "1.17.18-rc.1", "1.17.18+unreviewed"] {
            let rejected = project_acp_capability_pack(&pack_snapshot(
                OPENCODE_INITIALIZE_V1_FIXTURE,
                Some(version),
                None,
            ))
            .expect("OpenCode candidate");
            assert!(!rejected.active, "unreviewed OpenCode version {version}");
        }
    }

    #[test]
    fn codex_prompt_quota_projects_only_reviewed_typed_fixture_fields() {
        let response: agent_client_protocol::schema::v1::PromptResponse = serde_json::from_str(
            include_str!("../../tests/fixtures/codex_acp_prompt_quota.json"),
        )
        .expect("Codex prompt quota fixture");

        let projected = project_codex_prompt_quota(
            &codex_initialized(CODEX_ACP_REVIEWED_VERSION),
            response.meta.as_ref(),
        )
        .expect("reviewed quota schema")
        .expect("quota projection");
        let projected_value = serde_json::to_value(&projected).expect("projected quota value");

        assert_eq!(
            projected_value,
            json!({
                "schemaVersion": 1,
                "tokenCount": {
                    "totalTokens": 2500,
                    "inputTokens": 1500,
                    "cachedInputTokens": 500,
                    "outputTokens": 450,
                    "reasoningOutputTokens": 50
                },
                "modelUsage": [{
                    "model": "model-id",
                    "tokenCount": {
                        "totalTokens": 2500,
                        "inputTokens": 1500,
                        "cachedInputTokens": 500,
                        "outputTokens": 450,
                        "reasoningOutputTokens": 50
                    }
                }]
            })
        );
        assert!(!projected_value.to_string().contains("must-not-cross"));

        let mut state = AcpPeerStreamState::new(None, "local-quota".to_string());
        state.handle_prompt_usage(json!({ "totalTokens": 2500 }));
        state.handle_codex_prompt_quota(projected);
        assert_eq!(
            state
                .usage_update
                .as_ref()
                .and_then(|usage| usage.get("codexPromptQuota")),
            Some(&projected_value)
        );
        assert!(state.events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("acp_peer_codex_prompt_quota")
        }));

        assert!(
            project_codex_prompt_quota(&codex_initialized("1.1.3"), response.meta.as_ref())
                .expect("unreviewed version is not a schema error")
                .is_none(),
            "future versions must not activate the reviewed quota pack"
        );
    }

    #[test]
    fn codex_prompt_quota_rejects_unknown_and_unbounded_schema_without_echoing_it() {
        let mut unknown = json!({
            "quota": {
                "token_count": null,
                "model_usage": [],
                "secret": "must-not-cross"
            }
        })
        .as_object()
        .expect("metadata object")
        .clone();
        assert_eq!(
            project_codex_prompt_quota(
                &codex_initialized(CODEX_ACP_REVIEWED_VERSION),
                Some(&unknown),
            ),
            Err(CodexPromptQuotaRejection::InvalidSchema)
        );

        unknown.insert(
            "quota".to_string(),
            json!({
                "token_count": {
                    "totalTokens": 1,
                    "inputTokens": 1,
                    "cachedInputTokens": 0,
                    "outputTokens": 0,
                    "reasoningOutputTokens": 0
                },
                "model_usage": (0..=CODEX_QUOTA_MAX_MODEL_ENTRIES)
                    .map(|index| json!({
                        "model": format!("model-{index}"),
                        "token_count": {
                            "totalTokens": 1,
                            "inputTokens": 1,
                            "cachedInputTokens": 0,
                            "outputTokens": 0,
                            "reasoningOutputTokens": 0
                        }
                    }))
                    .collect::<Vec<_>>()
            }),
        );
        assert_eq!(
            project_codex_prompt_quota(
                &codex_initialized(CODEX_ACP_REVIEWED_VERSION),
                Some(&unknown),
            ),
            Err(CodexPromptQuotaRejection::BoundsExceeded)
        );
        assert_eq!(
            CodexPromptQuotaRejection::InvalidSchema.as_str(),
            "invalid_schema"
        );
        let mut state = AcpPeerStreamState::new(None, "local-rejected-quota".to_string());
        state.handle_codex_prompt_quota_rejection(CodexPromptQuotaRejection::InvalidSchema);
        let diagnostic = state.events.last().expect("bounded rejection diagnostic");
        assert_eq!(diagnostic["reason"], "invalid_schema");
        assert!(!diagnostic.to_string().contains("must-not-cross"));
    }
}
