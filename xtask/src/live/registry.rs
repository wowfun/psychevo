use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::Serialize;

pub(crate) const DEFAULT_SUITE: &str = "smoke";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LiveCheck {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) suites: &'static [&'static str],
    pub(crate) action: LiveCheckAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LiveCheckAction {
    DesktopNativeSmoke {
        provider_required: bool,
    },
    ProviderSmoke,
    PevoDoctorLive,
    CargoIgnoredTest {
        package: &'static str,
        test: &'static str,
        provider_required: bool,
    },
    DeterministicPlaywright {
        spec: &'static str,
        grep: &'static str,
    },
    Playwright {
        spec: &'static str,
        grep: &'static str,
        needs_opencode: bool,
        needs_skill_cwd: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct LiveProvider {
    pub(crate) id: &'static str,
    pub(crate) model: &'static str,
    pub(crate) credential_env: &'static [&'static str],
}

impl LiveProvider {
    pub(crate) fn parse(id: &str) -> Result<Self> {
        match id {
            "xiaomi-token-plan" => Ok(XIAOMI_TOKEN_PLAN),
            "deepseek" => Ok(DEEPSEEK),
            _ => bail!("unknown live provider: {id}"),
        }
    }
}

pub(crate) const XIAOMI_TOKEN_PLAN: LiveProvider = LiveProvider {
    id: "xiaomi-token-plan",
    model: "xiaomi-token-plan/mimo-v2.5-pro",
    credential_env: &[
        "XIAOMI_TOKEN_PLAN_API_KEY",
        "XIAOMI_TOKEN_PLAN_CN_API_KEY",
        "XIAOMI_API_KEY",
    ],
};

pub(crate) const DEEPSEEK: LiveProvider = LiveProvider {
    id: "deepseek",
    model: "deepseek/deepseek-chat",
    credential_env: &["DEEPSEEK_API_KEY"],
};

pub(crate) const LIVE_CHECKS: &[LiveCheck] = &[
    LiveCheck {
        id: "provider-smoke",
        description: "Run native provider smoke validation through pevo run",
        suites: &["smoke", "provider"],
        action: LiveCheckAction::ProviderSmoke,
    },
    LiveCheck {
        id: "pevo-doctor-live",
        description: "Run pevo doctor live provider diagnostics",
        suites: &["provider"],
        action: LiveCheckAction::PevoDoctorLive,
    },
    LiveCheck {
        id: "runtime-provider-read",
        description: "Run runtime ignored live provider read-tool validation",
        suites: &["provider"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-runtime",
            test: "live_xiaomi_token_plan_read_tool",
            provider_required: true,
        },
    },
    LiveCheck {
        id: "runtime-model-fetch",
        description: "Run runtime ignored live model catalog fetch validation",
        suites: &["provider"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-runtime",
            test: "live_xiaomi_token_plan_model_fetch",
            provider_required: true,
        },
    },
    LiveCheck {
        id: "gateway-automation-live",
        description: "Run gateway automation ignored live validation",
        suites: &["automation"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-gateway",
            test: "live_xiaomi_token_plan_automation_manual_run_completes",
            provider_required: true,
        },
    },
    LiveCheck {
        id: "codex-plugin-broker-live",
        description: "Read installed Codex plugins through the capability broker",
        suites: &["plugin"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-gateway",
            test: "server::codex_capability_broker::tests::live_codex_plugin_broker_lists_installed_plugins",
            provider_required: false,
        },
    },
    LiveCheck {
        id: "desktop-native-smoke-live",
        description: "Run native Desktop/Floating smoke validation without provider calls",
        suites: &["desktop"],
        action: LiveCheckAction::DesktopNativeSmoke {
            provider_required: false,
        },
    },
    LiveCheck {
        id: "desktop-floating-provider-live",
        description: "Run native Floating provider validation through Desktop",
        suites: &["desktop"],
        action: LiveCheckAction::DesktopNativeSmoke {
            provider_required: true,
        },
    },
    LiveCheck {
        id: "web-composer-draft-open-first-send",
        description: "Queue exactly one first Composer turn while a real Gateway draft open is pending",
        suites: &["web"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "keeps first send live across a pending atomic draft open on the real Gateway",
        },
    },
    LiveCheck {
        id: "web-composer-live",
        description: "Run Workbench live composer validation",
        suites: &["web"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/workbench.live.spec.ts",
            grep: "submits a real provider turn through the composer @live",
            needs_opencode: false,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "web-automation-live",
        description: "Run Workbench live automation GUI validation",
        suites: &["web", "automation"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/workbench.live.spec.ts",
            grep: "creates an automation through the live GUI without duplicating the final answer @live",
            needs_opencode: false,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "web-subagent-live",
        description: "Run Workbench live subagent GUI validation",
        suites: &["web"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/workbench.live.spec.ts",
            grep: "opens a live translate subagent session from the GUI @live",
            needs_opencode: false,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "web-skill-live",
        description: "Run Workbench live skill validation",
        suites: &["skill"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/live-skill.spec.ts",
            grep: "runs x-daily with sampled transcript assertions @live",
            needs_opencode: false,
            needs_skill_cwd: true,
        },
    },
    LiveCheck {
        id: "pevo-acp-server-live",
        description: "Run Psychevo ACP server live validation",
        suites: &["acp"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/pevo-acp-server-live.spec.ts",
            grep: "streams standard ACP updates, accepts model config, and reports usage @live",
            needs_opencode: false,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "opencode-acp-gui-live",
        description: "Run OpenCode ACP GUI live validation",
        suites: &["acp"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/opencode-acp-live.spec.ts",
            grep: "creates and uses OpenCode ACP from the GUI @live",
            needs_opencode: true,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "opencode-acp-delegate-live",
        description: "Run OpenCode ACP delegate live validation",
        suites: &["acp"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/opencode-acp-live.spec.ts",
            grep: "delegates @opencode through the native runtime @live",
            needs_opencode: true,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "opencode-acp-session-lifecycle-live",
        description: "Validate real OpenCode ACP list, fork, close, resume, and delete capability projection",
        suites: &["acp"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/opencode-acp-live.spec.ts",
            grep: "creates and uses OpenCode ACP from the GUI @live",
            needs_opencode: true,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "codex-acp-session-lifecycle-live",
        description: "Create, list, resume, close, and delete a test-owned real Codex ACP session",
        suites: &["acp"],
        action: LiveCheckAction::Playwright {
            spec: "apps/workbench/e2e/codex-acp-session-lifecycle-live.spec.ts",
            grep: "creates and deletes only its test-owned Codex ACP session @live",
            needs_opencode: false,
            needs_skill_cwd: false,
        },
    },
    LiveCheck {
        id: "agent-acp-gui-parity",
        description: "Run Codex ACP and OpenCode ACP through one deterministic Workbench path",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "runs Codex ACP and OpenCode ACP through one GUI control path @live",
        },
    },
    LiveCheck {
        id: "agent-acp-session-lifecycle",
        description: "Discover, import, fork, close, restore, and capability-gate ACP sessions through Workbench",
        suites: &["agents", "acp"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/agent-application-visual.spec.ts",
            grep: "imports Agent-owned sessions and renders negotiated lifecycle actions",
        },
    },
    LiveCheck {
        id: "agent-native-application-surface-parity",
        description: "Compare Native GUI and Channel binding, intent, and history through the public Thread contract",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "proves Native GUI and Channel equivalent binding intent and history semantics @live",
        },
    },
    LiveCheck {
        id: "agent-acp-capability-pack-version",
        description: "Activate only exact reviewed ACP capability-pack identities and versions",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "disables an incompatible reviewed ACP capability pack with an explicit diagnostic @live",
        },
    },
    LiveCheck {
        id: "agent-acp-history-reconnect",
        description: "Restore agent-owned ACP history and preserve MCP declarations across new/load",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "reuses one ACP process and restores agent-owned history without duplicate turns @live",
        },
    },
    LiveCheck {
        id: "agent-acp-process-ephemeral-history",
        description: "Expose process-owned partial ACP history and refuse fake recovery after restart",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "keeps process-ephemeral ACP history partial after restart and refuses fake recovery @live",
        },
    },
    LiveCheck {
        id: "agent-acp-channel-parity",
        description: "Apply Channel controls through the same ACP preference and delivery path",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "applies Channel controls through the same ACP preference and delivery path @live",
        },
    },
    LiveCheck {
        id: "agent-acp-client-callback-fidelity",
        description: "Validate ACP filesystem and permission callbacks with terminal explicitly unsupported",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "routes ACP filesystem permissions once through Channel and keeps terminal explicitly unsupported @live",
        },
    },
    LiveCheck {
        id: "agent-application-surface-parity",
        description: "Compare GUI and Channel binding, controls, delivery, and history through the public Thread contract",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "proves GUI and Channel equivalent binding control delivery and history semantics @live",
        },
    },
    LiveCheck {
        id: "agent-channel-interaction-once",
        description: "Consume ACP permission and elicitation Channel tokens exactly once",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "consumes Channel approve and answer tokens exactly once for ACP interactions @live",
        },
    },
    LiveCheck {
        id: "agent-acp-active-turn-next-control",
        description: "Queue an ACP control changed during an active turn for the next turn only",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "queues an active-turn ACP model change for the next turn without mutating the current turn @live",
        },
    },
    LiveCheck {
        id: "agent-acp-terminal-callback-fidelity",
        description: "Validate granted ACP terminal create, output, wait, kill, and release callbacks",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "runs the granted ACP terminal lifecycle through Channel approval and typed callbacks @live",
        },
    },
    LiveCheck {
        id: "agent-acp-unknown-delivery",
        description: "Reconcile an accepted ACP prompt without retrying unknown delivery",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "does not retry an ACP prompt after unknown delivery and reconciles from load @live",
        },
    },
    LiveCheck {
        id: "agent-managed-codex-offline",
        description: "Launch the pinned managed Codex ACP adapter from an offline absolute path",
        suites: &["agents"],
        action: LiveCheckAction::DeterministicPlaywright {
            spec: "apps/workbench/e2e/runtime-live.spec.ts",
            grep: "launches the pinned managed Codex ACP adapter from an offline absolute path @live",
        },
    },
];

pub(crate) const LIVE_SUITES: &[LiveSuite] = &[
    LiveSuite {
        id: "smoke",
        description: "Default live provider smoke",
    },
    LiveSuite {
        id: "provider",
        description: "Provider, catalog, and doctor live checks",
    },
    LiveSuite {
        id: "web",
        description: "Workbench live GUI checks",
    },
    LiveSuite {
        id: "skill",
        description: "Workbench live skill check",
    },
    LiveSuite {
        id: "desktop",
        description: "Native Desktop and Floating live checks",
    },
    LiveSuite {
        id: "acp",
        description: "Psychevo and OpenCode ACP live checks",
    },
    LiveSuite {
        id: "automation",
        description: "Gateway and Workbench automation live checks",
    },
    LiveSuite {
        id: "plugin",
        description: "Codex plugin and capability-broker live checks",
    },
    LiveSuite {
        id: "agents",
        description: "Deterministic Native and ACP Agent GUI, Channel, history, and delivery checks",
    },
    LiveSuite {
        id: "all",
        description: "All registered live checks",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct LiveSuite {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LiveSelection {
    pub(crate) checks: Vec<String>,
    pub(crate) suites: Vec<String>,
    pub(crate) all: bool,
    pub(crate) providers: Vec<String>,
}

pub(crate) fn check_by_id(id: &str) -> Option<&'static LiveCheck> {
    LIVE_CHECKS.iter().find(|check| check.id == id)
}

pub(crate) fn select_checks(selection: &LiveSelection) -> Result<Vec<&'static LiveCheck>> {
    if selection.all && (!selection.checks.is_empty() || !selection.suites.is_empty()) {
        bail!("--all cannot be combined with --check or --suite");
    }

    let mut selected = BTreeSet::new();
    if selection.all {
        selected.extend(LIVE_CHECKS.iter().map(|check| check.id.to_string()));
    } else if selection.checks.is_empty() && selection.suites.is_empty() {
        add_suite_checks(DEFAULT_SUITE, &mut selected)?;
    } else {
        for suite in &selection.suites {
            add_suite_checks(suite, &mut selected)?;
        }
        for check in &selection.checks {
            if check_by_id(check).is_none() {
                bail!("unknown live check: {check}");
            }
            selected.insert(check.clone());
        }
    }

    Ok(LIVE_CHECKS
        .iter()
        .filter(|check| selected.contains(check.id))
        .collect())
}

pub(crate) fn resolve_providers(provider_args: &[String]) -> Result<Vec<LiveProvider>> {
    let mut providers = Vec::new();
    let args = if provider_args.is_empty() {
        vec![XIAOMI_TOKEN_PLAN.id.to_string()]
    } else {
        provider_args.to_vec()
    };

    for raw in args {
        for part in raw
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let provider = LiveProvider::parse(part)?;
            if !providers
                .iter()
                .any(|item: &LiveProvider| item.id == provider.id)
            {
                providers.push(provider);
            }
        }
    }
    Ok(providers)
}

fn add_suite_checks(suite: &str, selected: &mut BTreeSet<String>) -> Result<()> {
    if suite == "all" {
        selected.extend(LIVE_CHECKS.iter().map(|check| check.id.to_string()));
        return Ok(());
    }
    if !LIVE_SUITES.iter().any(|item| item.id == suite) {
        bail!("unknown live suite: {suite}");
    }
    selected.extend(
        LIVE_CHECKS
            .iter()
            .filter(|check| check.suites.contains(&suite))
            .map(|check| check.id.to_string()),
    );
    Ok(())
}

pub(crate) fn command_for_plan(check: &LiveCheck) -> Vec<String> {
    match check.action {
        LiveCheckAction::DesktopNativeSmoke { provider_required } => vec![
            "xtask-internal".to_string(),
            "desktop-native-smoke".to_string(),
            format!("provider-required={provider_required}"),
        ],
        LiveCheckAction::ProviderSmoke => {
            vec!["xtask-internal".to_string(), "provider-smoke".to_string()]
        }
        LiveCheckAction::PevoDoctorLive => {
            vec!["xtask-internal".to_string(), "pevo-doctor-live".to_string()]
        }
        LiveCheckAction::CargoIgnoredTest { package, test, .. } => vec![
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            package.to_string(),
            test.to_string(),
            "--".to_string(),
            "--ignored".to_string(),
            "--exact".to_string(),
        ],
        LiveCheckAction::DeterministicPlaywright { spec, grep } => vec![
            "xtask-internal".to_string(),
            "playwright-deterministic".to_string(),
            spec.to_string(),
            "--grep".to_string(),
            grep.to_string(),
        ],
        LiveCheckAction::Playwright { spec, grep, .. } => vec![
            "xtask-internal".to_string(),
            "playwright-live".to_string(),
            spec.to_string(),
            "--grep".to_string(),
            grep.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_selection_is_smoke() {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: Vec::new(),
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        assert_eq!(
            checks.iter().map(|check| check.id).collect::<Vec<_>>(),
            vec!["provider-smoke"]
        );
    }

    #[test]
    fn suite_and_check_selection_is_deduped_in_registry_order() {
        let checks = select_checks(&LiveSelection {
            checks: vec!["provider-smoke".to_string()],
            suites: vec!["web".to_string(), "automation".to_string()],
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        assert_eq!(
            checks.iter().map(|check| check.id).collect::<Vec<_>>(),
            vec![
                "provider-smoke",
                "gateway-automation-live",
                "web-composer-draft-open-first-send",
                "web-composer-live",
                "web-automation-live",
                "web-subagent-live",
            ]
        );
    }

    #[test]
    fn desktop_suite_includes_native_and_provider_checks() {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: vec!["desktop".to_string()],
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        assert_eq!(
            checks.iter().map(|check| check.id).collect::<Vec<_>>(),
            vec![
                "desktop-native-smoke-live",
                "desktop-floating-provider-live"
            ]
        );
    }

    #[test]
    fn plugin_suite_selects_the_read_only_codex_broker_probe() {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: vec!["plugin".to_string()],
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        assert_eq!(
            checks.iter().map(|check| check.id).collect::<Vec<_>>(),
            vec!["codex-plugin-broker-live"]
        );
        assert!(matches!(
            checks[0].action,
            LiveCheckAction::CargoIgnoredTest {
                provider_required: false,
                ..
            }
        ));
    }

    #[test]
    fn desktop_provider_live_is_planned_as_provider_backed() {
        let check = check_by_id("desktop-floating-provider-live").expect("desktop check");
        assert_eq!(
            command_for_plan(check),
            vec![
                "xtask-internal".to_string(),
                "desktop-native-smoke".to_string(),
                "provider-required=true".to_string(),
            ]
        );
    }

    #[test]
    fn all_expands_to_every_registered_check() {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: Vec::new(),
            all: true,
            providers: Vec::new(),
        })
        .expect("checks");
        assert_eq!(checks.len(), LIVE_CHECKS.len());
    }

    #[test]
    fn acp_suite_includes_psychevo_and_opencode_checks() {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: vec!["acp".to_string()],
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        let ids = checks.iter().map(|check| check.id).collect::<BTreeSet<_>>();
        for expected in [
            "pevo-acp-server-live",
            "opencode-acp-gui-live",
            "opencode-acp-delegate-live",
            "opencode-acp-session-lifecycle-live",
            "codex-acp-session-lifecycle-live",
            "agent-acp-session-lifecycle",
        ] {
            assert!(ids.contains(expected), "ACP suite is missing {expected}");
        }
    }

    #[test]
    fn agents_suite_covers_native_acp_gui_channel_packs_callbacks_history_delivery_and_managed_codex()
     {
        let checks = select_checks(&LiveSelection {
            checks: Vec::new(),
            suites: vec!["agents".to_string()],
            all: false,
            providers: Vec::new(),
        })
        .expect("checks");
        let ids = checks.iter().map(|check| check.id).collect::<BTreeSet<_>>();
        for expected in [
            "agent-acp-gui-parity",
            "agent-acp-session-lifecycle",
            "agent-native-application-surface-parity",
            "agent-acp-capability-pack-version",
            "agent-acp-history-reconnect",
            "agent-acp-process-ephemeral-history",
            "agent-acp-channel-parity",
            "agent-acp-client-callback-fidelity",
            "agent-application-surface-parity",
            "agent-channel-interaction-once",
            "agent-acp-active-turn-next-control",
            "agent-acp-terminal-callback-fidelity",
            "agent-acp-unknown-delivery",
            "agent-managed-codex-offline",
        ] {
            assert!(ids.contains(expected), "Agents suite is missing {expected}");
        }
        assert!(checks.iter().all(|check| matches!(
            check.action,
            LiveCheckAction::DeterministicPlaywright { .. }
        )));
    }

    #[test]
    fn deterministic_agent_plan_does_not_claim_a_direct_or_latest_runtime_command() {
        let check = check_by_id("agent-acp-gui-parity").expect("agent check");
        let command = command_for_plan(check);
        assert_eq!(
            command[0..2],
            ["xtask-internal", "playwright-deterministic"]
        );
        assert!(
            !command
                .iter()
                .any(|part| matches!(part.as_str(), "opencode" | "codex" | "npx" | "latest"))
        );
    }

    #[test]
    fn unknown_check_is_rejected() {
        let err = select_checks(&LiveSelection {
            checks: vec!["missing".to_string()],
            suites: Vec::new(),
            all: false,
            providers: Vec::new(),
        })
        .expect_err("unknown check");
        assert!(err.to_string().contains("unknown live check"));
    }

    #[test]
    fn provider_cli_selection_defaults_and_dedupes() {
        assert_eq!(
            resolve_providers(&[]).expect("default providers"),
            vec![XIAOMI_TOKEN_PLAN]
        );
        assert_eq!(
            resolve_providers(&[
                "deepseek".to_string(),
                "xiaomi-token-plan,deepseek".to_string(),
            ])
            .expect("providers"),
            vec![DEEPSEEK, XIAOMI_TOKEN_PLAN]
        );
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let err = resolve_providers(&["unknown".to_string()]).expect_err("provider");
        assert!(err.to_string().contains("unknown live provider"));
    }
}
