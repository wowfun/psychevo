use std::path::Path;

use anyhow::{Result, anyhow, bail};

use super::artifacts::display_path;
use super::model::{
    CiEnvironmentOutput, PlanOutput, ProfileKind, ProfileSummary, StepPlanOutput, WorkflowProfile,
    WorkflowStep, WorkflowStepAction, profile_summary,
};
use crate::live::LiveEnvMode;

const CHANGED_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "diff-check",
        description: "Check whitespace errors in the current diff",
        action: WorkflowStepAction::Command(&["git", "diff", "--check"]),
        live: false,
    },
    WorkflowStep {
        id: "rust-format",
        description: "Check Rust formatting",
        action: WorkflowStepAction::Command(&["cargo", "fmt", "--all", "--check"]),
        live: false,
    },
];

const RUST_BROAD_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "rust-format",
        description: "Check Rust formatting",
        action: WorkflowStepAction::Command(&["cargo", "fmt", "--all", "--check", "--quiet"]),
        live: false,
    },
    WorkflowStep {
        id: "rust-clippy",
        description: "Run Rust clippy for all workspace targets",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--quiet",
            "--",
            "-D",
            "warnings",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "rust-tests",
        description: "Run Rust tests for all workspace targets",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "--workspace",
            "--all-targets",
            "--quiet",
        ]),
        live: false,
    },
];

const DESKTOP_RUST_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "desktop-format",
        description: "Check Desktop Rust formatting",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "fmt",
            "--manifest-path",
            "apps/desktop/src-tauri/Cargo.toml",
            "--all",
            "--check",
            "--quiet",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "desktop-clippy",
        description: "Run Desktop Rust clippy for the shipped native runtime",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "clippy",
            "--manifest-path",
            "apps/desktop/src-tauri/Cargo.toml",
            "--features",
            "native-runtime",
            "--all-targets",
            "--quiet",
            "--",
            "-D",
            "warnings",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "desktop-tests",
        description: "Run Desktop Rust tests for the shipped native runtime",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "--manifest-path",
            "apps/desktop/src-tauri/Cargo.toml",
            "--features",
            "native-runtime",
            "--all-targets",
            "--quiet",
        ]),
        live: false,
    },
];

const WEB_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "workspace-tests",
        description: "Run all JavaScript workspace unit tests sequentially",
        action: WorkflowStepAction::Command(&["pnpm", "--workspace-concurrency=1", "-r", "test"]),
        live: false,
    },
    WorkflowStep {
        id: "workspace-typecheck",
        description: "Typecheck all JavaScript workspaces",
        action: WorkflowStepAction::Command(&["pnpm", "-r", "typecheck"]),
        live: false,
    },
    WorkflowStep {
        id: "workspace-builds",
        description: "Build all JavaScript workspaces, including Workbench and Desktop",
        action: WorkflowStepAction::Command(&["pnpm", "-r", "build"]),
        live: false,
    },
];

const VISUAL_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "tui-vhs-demo",
        description: "Capture deterministic TUI visual diagnostics",
        action: WorkflowStepAction::TuiVhsDemo,
        live: false,
    },
    WorkflowStep {
        id: "workbench-visual",
        description: "Capture deterministic Workbench Playwright visual diagnostics",
        action: WorkflowStepAction::WorkbenchVisual,
        live: false,
    },
    WorkflowStep {
        id: "desktop-visual",
        description: "Capture deterministic Desktop/Floating visual diagnostics",
        action: WorkflowStepAction::DesktopVisual,
        live: false,
    },
];

const SURFACE_PROFILE_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "workbench-build",
        description: "Build Workbench for deterministic surface profiling",
        action: WorkflowStepAction::Command(&["pnpm", "--filter", "@psychevo/workbench", "build"]),
        live: false,
    },
    WorkflowStep {
        id: "pevo-debug-build",
        description: "Build pevo with the fullscreen TUI profiling probe",
        action: WorkflowStepAction::Command(&["cargo", "build", "-p", "psychevo-cli", "--quiet"]),
        live: false,
    },
    WorkflowStep {
        id: "surface-comparison",
        description: "Profile the same deterministic Native turn through TUI and Workbench",
        action: WorkflowStepAction::SurfaceProfile,
        live: false,
    },
];

const LIVE_STEPS: &[WorkflowStep] = &[WorkflowStep {
    id: "single-provider-live",
    description: "Run explicit live provider smoke validation",
    action: WorkflowStepAction::SingleProviderLive,
    live: true,
}];

const PACKAGE_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "build-cli-release",
        description: "Build release CLI artifact",
        action: WorkflowStepAction::Command(&["cargo", "build", "-p", "psychevo-cli", "--release"]),
        live: false,
    },
    WorkflowStep {
        id: "build-workbench",
        description: "Build Workbench artifact",
        action: WorkflowStepAction::Command(&["pnpm", "--filter", "@psychevo/workbench", "build"]),
        live: false,
    },
    WorkflowStep {
        id: "checksum-local-artifacts",
        description: "Write local checksums without publishing artifacts",
        action: WorkflowStepAction::Command(&[
            "sh",
            "-c",
            "mkdir -p \"$PSYCHEVO_CI_ARTIFACT_ROOT/package\" && if [ -x target/release/pevo ]; then sha256sum target/release/pevo > \"$PSYCHEVO_CI_ARTIFACT_ROOT/package/pevo.sha256\"; fi",
        ]),
        live: false,
    },
];

const PROFILES: &[WorkflowProfile] = &[
    WorkflowProfile {
        id: "changed",
        description: "Lightweight local checks for the current checkout",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: CHANGED_STEPS,
    },
    WorkflowProfile {
        id: "rust-broad",
        description: "Rust workspace broad deterministic gate",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: RUST_BROAD_STEPS,
    },
    WorkflowProfile {
        id: "desktop-rust",
        description: "Desktop native-runtime Rust gate",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: DESKTOP_RUST_STEPS,
    },
    WorkflowProfile {
        id: "web",
        description: "Client, Workbench, and Desktop web-surface gates",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: WEB_STEPS,
    },
    WorkflowProfile {
        id: "visual",
        description: "Deterministic visual diagnostics",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: VISUAL_STEPS,
    },
    WorkflowProfile {
        id: "surface-profile",
        description: "Deterministic TUI versus Workbench journey profiling",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: SURFACE_PROFILE_STEPS,
    },
    WorkflowProfile {
        id: "live",
        description: "Explicit live provider validation",
        kind: ProfileKind::Ci,
        live: true,
        artifact_only: false,
        steps: LIVE_STEPS,
    },
    WorkflowProfile {
        id: "package",
        description: "Artifact-only delivery preparation",
        kind: ProfileKind::Cd,
        live: false,
        artifact_only: true,
        steps: PACKAGE_STEPS,
    },
];

pub(crate) fn profile_summaries() -> Vec<ProfileSummary> {
    PROFILES.iter().map(profile_summary).collect()
}

pub(crate) fn find_profile(id: &str) -> Result<&'static WorkflowProfile> {
    PROFILES
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| anyhow!("unknown CI/CD profile: {id}"))
}

pub(crate) fn plan_profile(id: &str, live_env: Option<LiveEnvMode>) -> Result<PlanOutput> {
    let profile = find_profile(id)?;
    plan_for_profile_with_env(profile, None, live_env)
}

pub(crate) fn plan_for_profile_with_env(
    profile: &WorkflowProfile,
    artifact_root: Option<&Path>,
    live_env: Option<LiveEnvMode>,
) -> Result<PlanOutput> {
    if live_env.is_some() && !profile.live {
        bail!("--live-env is only valid for live CI/CD profiles");
    }
    Ok(PlanOutput {
        profile: profile_summary(profile),
        environment: profile.live.then_some(CiEnvironmentOutput {
            mode: live_env.unwrap_or_default(),
        }),
        artifact_root: artifact_root.map(display_path),
        steps: profile.steps.iter().map(step_plan).collect(),
    })
}

fn step_plan(step: &WorkflowStep) -> StepPlanOutput {
    StepPlanOutput {
        id: step.id,
        description: step.description,
        command: step
            .action
            .command_for_plan()
            .iter()
            .map(|part| (*part).to_string())
            .collect(),
        live: step.live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_initial_profiles() {
        let ids: Vec<_> = profile_summaries()
            .into_iter()
            .map(|profile| profile.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "changed",
                "rust-broad",
                "desktop-rust",
                "web",
                "visual",
                "surface-profile",
                "live",
                "package"
            ]
        );
    }

    #[test]
    fn changed_plan_is_machine_readable_without_running_steps() {
        let plan = plan_profile("changed", None).expect("changed profile");
        assert_eq!(plan.profile.id, "changed");
        assert!(plan.artifact_root.is_none());
        assert!(plan.steps.iter().any(|step| step.id == "diff-check"));
        let json = serde_json::to_value(&plan).expect("plan json");
        assert_eq!(json["profile"]["id"], "changed");
    }

    #[test]
    fn visual_plan_uses_runner_owned_visual_steps() {
        let plan = plan_profile("visual", None).expect("visual profile");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].id, "tui-vhs-demo");
        assert_eq!(
            plan.steps[0].command,
            vec!["xtask-internal", "tui-vhs-demo"]
        );
        assert_eq!(plan.steps[1].id, "workbench-visual");
        assert_eq!(
            plan.steps[1].command,
            vec!["xtask-internal", "workbench-visual"]
        );
        assert_eq!(plan.steps[2].id, "desktop-visual");
        assert_eq!(
            plan.steps[2].command,
            vec!["xtask-internal", "desktop-visual"]
        );
    }

    #[test]
    fn web_plan_covers_all_workspace_tests_typechecks_and_builds() {
        let plan = plan_profile("web", None).expect("web profile");
        let ids = plan.steps.iter().map(|step| step.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["workspace-tests", "workspace-typecheck", "workspace-builds"]
        );
        assert_eq!(
            plan.steps[0].command,
            vec!["pnpm", "--workspace-concurrency=1", "-r", "test"]
        );
    }

    #[test]
    fn surface_profile_plan_builds_both_surfaces_then_runs_owned_profiler() {
        let plan = plan_profile("surface-profile", None).expect("surface profile");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].id, "workbench-build");
        assert_eq!(plan.steps[1].id, "pevo-debug-build");
        assert_eq!(plan.steps[2].id, "surface-comparison");
        assert_eq!(
            plan.steps[2].command,
            vec!["xtask-internal", "surface-profile"]
        );
    }

    #[test]
    fn live_plan_uses_runner_owned_live_step() {
        let plan = plan_profile("live", None).expect("live profile");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(
            plan.environment.expect("environment").mode,
            LiveEnvMode::Shared
        );
        let step = &plan.steps[0];
        assert_eq!(step.id, "single-provider-live");
        assert_eq!(step.command, vec!["xtask-internal", "single-provider-live"]);
        assert!(step.live);
    }

    #[test]
    fn live_plan_accepts_isolated_env_mode() {
        let plan = plan_profile("live", Some(LiveEnvMode::Isolated)).expect("live profile");
        assert_eq!(
            plan.environment.expect("environment").mode,
            LiveEnvMode::Isolated
        );
    }

    #[test]
    fn non_live_plan_rejects_live_env_mode() {
        let err = plan_profile("changed", Some(LiveEnvMode::Isolated))
            .expect_err("non-live profile should reject live-env");
        assert!(err.to_string().contains("--live-env"));
    }

    #[test]
    fn package_plan_is_artifact_only_and_has_no_publish_steps() {
        let plan = plan_profile("package", None).expect("package profile");
        assert_eq!(plan.profile.kind, ProfileKind::Cd);
        assert!(plan.profile.artifact_only);
        let forbidden = ["publish", "deploy", "upload", "tag", "push"];
        for step in plan.steps {
            let command = step.command.join(" ").to_ascii_lowercase();
            assert!(
                !forbidden.iter().any(|word| command.contains(word)),
                "package step '{}' contains forbidden delivery verb in command: {}",
                step.id,
                command
            );
        }
    }

    #[test]
    fn rust_broad_cargo_steps_use_quiet_output() {
        let plan = plan_profile("rust-broad", None).expect("rust-broad profile");
        for step in plan.steps {
            assert!(
                step.command.iter().any(|part| part == "--quiet"),
                "step '{}' should quiet normal cargo output: {:?}",
                step.id,
                step.command
            );
        }
    }

    #[test]
    fn desktop_rust_plan_validates_the_shipped_native_runtime() {
        let plan = plan_profile("desktop-rust", None).expect("desktop-rust profile");
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec!["desktop-format", "desktop-clippy", "desktop-tests"]
        );

        let manifest = "apps/desktop/src-tauri/Cargo.toml";
        assert_eq!(
            plan.steps[0].command,
            vec![
                "cargo",
                "fmt",
                "--manifest-path",
                manifest,
                "--all",
                "--check",
                "--quiet"
            ]
        );
        for step in &plan.steps[1..] {
            assert!(
                step.command
                    .windows(2)
                    .any(|parts| parts == ["--manifest-path".to_string(), manifest.to_string()])
            );
            assert!(
                step.command
                    .windows(2)
                    .any(|parts| parts == ["--features".to_string(), "native-runtime".to_string()])
            );
            assert!(step.command.iter().any(|part| part == "--all-targets"));
            assert!(!step.command.iter().any(|part| part == "wdio-test"));
        }
        assert!(plan.steps[1].command.ends_with(&[
            "--".to_string(),
            "-D".to_string(),
            "warnings".to_string()
        ]));
    }
}
