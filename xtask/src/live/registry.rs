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
    ProviderSmoke,
    PevoDoctorLive,
    CargoIgnoredTest {
        package: &'static str,
        test: &'static str,
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
        },
    },
    LiveCheck {
        id: "runtime-model-fetch",
        description: "Run runtime ignored live model catalog fetch validation",
        suites: &["provider"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-runtime",
            test: "live_xiaomi_token_plan_model_fetch",
        },
    },
    LiveCheck {
        id: "gateway-automation-live",
        description: "Run gateway automation ignored live validation",
        suites: &["automation"],
        action: LiveCheckAction::CargoIgnoredTest {
            package: "psychevo-gateway",
            test: "live_xiaomi_token_plan_automation_manual_run_completes",
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
            grep: "opens live translate subagent sessions from the GUI @live",
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
        id: "acp",
        description: "OpenCode ACP live checks",
    },
    LiveSuite {
        id: "automation",
        description: "Gateway and Workbench automation live checks",
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
        LiveCheckAction::ProviderSmoke => {
            vec!["xtask-internal".to_string(), "provider-smoke".to_string()]
        }
        LiveCheckAction::PevoDoctorLive => {
            vec!["xtask-internal".to_string(), "pevo-doctor-live".to_string()]
        }
        LiveCheckAction::CargoIgnoredTest { package, test } => vec![
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            package.to_string(),
            test.to_string(),
            "--".to_string(),
            "--ignored".to_string(),
            "--exact".to_string(),
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
                "web-composer-live",
                "web-automation-live",
                "web-subagent-live",
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
