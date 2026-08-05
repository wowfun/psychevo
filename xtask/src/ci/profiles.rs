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

const SDK_ARCHITECTURE_STEP: WorkflowStep = WorkflowStep {
    id: "sdk-architecture",
    description: "Check SDK dependency direction, publication boundary, and semantic API use",
    action: WorkflowStepAction::SdkArchitecture,
    live: false,
};

const RUST_SDK_DEFAULT_SURFACE_STEP: WorkflowStep = WorkflowStep {
    id: "rust-sdk-default-surface",
    description: "Compile the published Framework with its empty default feature set",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "check",
        "-p",
        "psychevo",
        "--no-default-features",
        "--all-targets",
        "--quiet",
    ]),
    live: false,
};

const GATEWAY_PROTOCOL_CHECK_STEP: WorkflowStep = WorkflowStep {
    id: "gateway-protocol-check",
    description: "Check generated Gateway protocol bindings",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "--quiet",
        "xtask",
        "gateway-protocol",
        "generate",
        "--check",
    ]),
    live: false,
};

const RUST_APP_SERVER_CONTRACTS_STEP: WorkflowStep = WorkflowStep {
    id: "rust-app-server-contracts",
    description: "Run the Rust App Server compaction wire contract",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "test",
        "-p",
        "psychevo-gateway-protocol",
        "-p",
        "psychevo-gateway",
        "thread_compact",
        "--quiet",
    ]),
    live: false,
};

const TYPESCRIPT_SDK_CONTRACTS_STEP: WorkflowStep = WorkflowStep {
    id: "typescript-sdk-contracts",
    description: "Run generated protocol and TypeScript client contract tests",
    action: WorkflowStepAction::Command(&[
        "pnpm",
        "--filter",
        "@psychevo/protocol",
        "--filter",
        "@psychevo/client",
        "test",
    ]),
    live: false,
};

const PYTHON_SDK_TEST_STEP: WorkflowStep = WorkflowStep {
    id: "test-python-sdk-client",
    description: "Run every Python SDK client and transport test",
    action: WorkflowStepAction::Command(&[
        "uv",
        "run",
        "--no-project",
        "--with",
        "websockets>=15,<16",
        "python",
        "-m",
        "unittest",
        "discover",
        "-s",
        "python/psychevo/tests",
        "-v",
    ]),
    live: false,
};

const PYTHON_PACKAGE_CONTRACTS_STEP: WorkflowStep = WorkflowStep {
    id: "verify-python-package-contracts",
    description: "Build and install all Python wheel and sdist contracts",
    action: WorkflowStepAction::Command(&[
        "python",
        "-m",
        "unittest",
        "discover",
        "-s",
        "python/tests",
        "-v",
    ]),
    live: false,
};

const WORKFLOW_ACTION_PINS_STEP: WorkflowStep = WorkflowStep {
    id: "workflow-action-pins",
    description: "Check that third-party workflow actions use full commit pins",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "test",
        "-p",
        "psychevo-xtask",
        "workflow_action_references_are_full_commit_pins",
        "--quiet",
    ]),
    live: false,
};

const MAIN_ARTIFACT_SMOKE_STEP: WorkflowStep = WorkflowStep {
    id: "smoke-installed-app-server-artifacts",
    description: "Build, install, and exercise the Linux Python SDK and App Server artifacts",
    action: WorkflowStepAction::Command(&[
        "python",
        "python/tests/installed_artifact_smoke.py",
        "--app-server-only",
    ]),
    live: false,
};

const INSTALLED_ARTIFACT_SMOKE_STEP: WorkflowStep = WorkflowStep {
    id: "smoke-installed-python-artifacts",
    description: "Run the installed SDK through the real bundled App Server and fake provider",
    action: WorkflowStepAction::Command(&["python", "python/tests/installed_artifact_smoke.py"]),
    live: false,
};

const NATIVE_ARTIFACT_SMOKE_STEP: WorkflowStep = WorkflowStep {
    id: "smoke-native-release-artifacts",
    description: "Start the exact release CLI and launchable host Desktop artifacts",
    action: WorkflowStepAction::Command(&["python", "scripts/package_artifact_smoke.py"]),
    live: false,
};

const VERIFY_SUPPLY_CHAIN_TOOLS_STEP: WorkflowStep = WorkflowStep {
    id: "verify-supply-chain-tools",
    description: "Verify the repository-pinned supply-chain scanner versions",
    action: WorkflowStepAction::Command(&["python", "scripts/supply_chain_tools.py", "verify"]),
    live: false,
};

const RUST_DEPENDENCY_POLICY_STEP: WorkflowStep = WorkflowStep {
    id: "rust-dependency-policy",
    description: "Check Rust advisories, bans, licenses, and dependency sources",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "deny",
        "--locked",
        "check",
        "--config",
        "deny.toml",
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]),
    live: false,
};

const PNPM_PRODUCTION_ADVISORIES_STEP: WorkflowStep = WorkflowStep {
    id: "pnpm-production-advisories",
    description: "Audit production pnpm dependencies at every advisory severity",
    action: WorkflowStepAction::Command(&["pnpm", "audit", "--prod", "--audit-level", "low"]),
    live: false,
};

const RUST_UNUSED_DEPENDENCIES_STEP: WorkflowStep = WorkflowStep {
    id: "rust-unused-dependencies",
    description: "Reject unused Rust dependencies across every workspace crate",
    action: WorkflowStepAction::Command(&["cargo-machete", "--with-metadata"]),
    live: false,
};

const PNPM_DEPENDENCY_HYGIENE_STEP: WorkflowStep = WorkflowStep {
    id: "pnpm-dependency-hygiene",
    description: "Reject unused, unlisted, and unresolved pnpm dependencies",
    action: WorkflowStepAction::Command(&["pnpm", "run", "dependency:check"]),
    live: false,
};

const COMMITTED_SECRET_SCAN_STEP: WorkflowStep = WorkflowStep {
    id: "committed-secret-scan",
    description: "Scan all available committed Git history for secrets",
    action: WorkflowStepAction::Command(&[
        "gitleaks",
        "git",
        "--config",
        ".gitleaks.toml",
        "--redact",
        "--no-banner",
        "--log-opts=--all",
        ".",
    ]),
    live: false,
};

const RUST_CORE_CHECK_STEP: WorkflowStep = WorkflowStep {
    id: "rust-core-check",
    description: "Check the CLI core graph without default features",
    action: WorkflowStepAction::Command(&[
        "cargo",
        "check",
        "-p",
        "psychevo-cli",
        "--no-default-features",
        "--quiet",
    ]),
    live: false,
};

const RUST_FORMAT_STEP: WorkflowStep = WorkflowStep {
    id: "rust-format",
    description: "Check Rust formatting",
    action: WorkflowStepAction::Command(&["cargo", "fmt", "--all", "--check", "--quiet"]),
    live: false,
};

const RUST_CLIPPY_STEP: WorkflowStep = WorkflowStep {
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
};

const RUST_TEST_STEP: WorkflowStep = WorkflowStep {
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
};

const RUST_CHECKS_STEPS: &[WorkflowStep] = &[
    SDK_ARCHITECTURE_STEP,
    RUST_SDK_DEFAULT_SURFACE_STEP,
    GATEWAY_PROTOCOL_CHECK_STEP,
    RUST_CORE_CHECK_STEP,
    RUST_FORMAT_STEP,
    RUST_CLIPPY_STEP,
];

const RUST_TEST_STEPS: &[WorkflowStep] = &[RUST_TEST_STEP];

const RUST_BROAD_STEPS: &[WorkflowStep] = &[
    SDK_ARCHITECTURE_STEP,
    RUST_SDK_DEFAULT_SURFACE_STEP,
    GATEWAY_PROTOCOL_CHECK_STEP,
    RUST_CORE_CHECK_STEP,
    RUST_FORMAT_STEP,
    RUST_CLIPPY_STEP,
    RUST_TEST_STEP,
];

const SDK_CONTRACTS_STEPS: &[WorkflowStep] = &[
    SDK_ARCHITECTURE_STEP,
    RUST_SDK_DEFAULT_SURFACE_STEP,
    GATEWAY_PROTOCOL_CHECK_STEP,
    RUST_APP_SERVER_CONTRACTS_STEP,
    TYPESCRIPT_SDK_CONTRACTS_STEP,
    PYTHON_SDK_TEST_STEP,
    PYTHON_PACKAGE_CONTRACTS_STEP,
    WORKFLOW_ACTION_PINS_STEP,
];

const MAIN_ARTIFACT_SMOKE_STEPS: &[WorkflowStep] = &[MAIN_ARTIFACT_SMOKE_STEP];

const SUPPLY_CHAIN_STEPS: &[WorkflowStep] = &[
    VERIFY_SUPPLY_CHAIN_TOOLS_STEP,
    RUST_DEPENDENCY_POLICY_STEP,
    RUST_UNUSED_DEPENDENCIES_STEP,
    PNPM_PRODUCTION_ADVISORIES_STEP,
    PNPM_DEPENDENCY_HYGIENE_STEP,
    COMMITTED_SECRET_SCAN_STEP,
];

const NON_FUNCTIONAL_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "measure-framework-footprint",
        description: "Measure and enforce Framework dependency and same-run compile budgets",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/non_functional_budgets.py",
            "framework",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "build-budgeted-cli-release",
        description: "Build the release CLI in the artifact-owned budget target",
        action: WorkflowStepAction::ArtifactCommand {
            command: &[
                "cargo",
                "build",
                "--locked",
                "--release",
                "-p",
                "psychevo-cli",
                "--bin",
                "pevo",
            ],
            target_dir: "cli-target",
        },
        live: false,
    },
    WorkflowStep {
        id: "release-cli-startup-budget",
        description: "Measure first-process and repeated-process release CLI startup",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/non_functional_budgets.py",
            "cli-startup",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "build-budgeted-workbench",
        description: "Build the production Workbench before startup-byte measurement",
        action: WorkflowStepAction::Command(&["pnpm", "--filter", "@psychevo/workbench", "build"]),
        live: false,
    },
    WorkflowStep {
        id: "build-budgeted-desktop",
        description: "Build the shipped Linux Desktop executable for footprint measurement",
        action: WorkflowStepAction::ArtifactCommand {
            command: &[
                "pnpm",
                "--filter",
                "@psychevo/desktop",
                "exec",
                "tauri",
                "build",
                "--features",
                "native-runtime",
                "--no-bundle",
            ],
            target_dir: "non-functional-desktop-target",
        },
        live: false,
    },
    INSTALLED_ARTIFACT_SMOKE_STEP,
    WorkflowStep {
        id: "gateway-first-result-budget",
        description: "Enforce initialized GUI pre-provider and first-result overhead",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "-p",
            "psychevo-gateway",
            "initialized_gui_first_token_overhead_stays_close_to_direct_gateway_dispatch",
            "--quiet",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "gateway-idle-database-budget",
        description: "Prove an idle Shell coordinator performs no SQLite operations",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "-p",
            "psychevo-gateway",
            "shell_scheduler_parks_without_tracked_activity_and_track_wakes_foreign_control",
            "--quiet",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "gateway-heartbeat-batch-budget",
        description: "Prove the full Shell admission limit uses one heartbeat transaction",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "-p",
            "psychevo-gateway",
            "one_dispatcher_and_one_manual_heartbeat_transaction_cover_the_full_shell_limit",
            "--quiet",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "gateway-retained-event-budget",
        description: "Measure retained-event batch persistence latency and time per event",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "test",
            "-p",
            "psychevo-gateway",
            "retained_event_ingress_stays_within_the_persistence_budget",
            "--quiet",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "workbench-startup-budget",
        description: "Measure Workbench startup JavaScript and optional preview asset bytes",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/non_functional_budgets.py",
            "workbench",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "enforce-linux-artifact-budgets",
        description: "Enforce Linux CLI, Desktop, and installed Python wheel size budgets",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/non_functional_budgets.py",
            "artifacts",
        ]),
        live: false,
    },
];

const INSTRUMENTATION_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "verify-instrumentation-harness",
        description: "Verify clean-output, failure, timeout, and exact-command harness contracts",
        action: WorkflowStepAction::Command(&[
            "python",
            "-m",
            "unittest",
            "discover",
            "-s",
            "scripts/tests",
            "-p",
            "test_high_risk_instrumentation.py",
            "-v",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "verify-instrumentation-tools",
        description: "Verify pinned coverage, nightly, and Miri tools",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/high_risk_instrumentation.py",
            "verify",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "targeted-rust-coverage",
        description: "Capture Framework, Gateway, and protocol library coverage",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/high_risk_instrumentation.py",
            "coverage",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "deterministic-boundary-contracts",
        description: "Run finite protocol, stream, UTF-8, and tool-argument boundary matrices",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/high_risk_instrumentation.py",
            "deterministic",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "protocol-miri",
        description: "Run pure Gateway protocol contracts under pinned Miri",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/high_risk_instrumentation.py",
            "miri",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "gateway-lifecycle-asan",
        description: "Run the selected Gateway lifecycle contract under AddressSanitizer",
        action: WorkflowStepAction::Command(&[
            "python",
            "scripts/high_risk_instrumentation.py",
            "asan",
        ]),
        live: false,
    },
];

const DESKTOP_RUST_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        id: "desktop-manifest-parity",
        description: "Check root and Desktop Cargo manifest parity",
        action: WorkflowStepAction::DesktopManifestParity,
        live: false,
    },
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
    WorkflowStep {
        id: "critical-browser-journey",
        description: "Run the deterministic critical first-Turn browser journey",
        action: WorkflowStepAction::WorkbenchCriticalJourney,
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
        description: "Run the complete deterministic Workbench Playwright inventory",
        action: WorkflowStepAction::WorkbenchVisual,
        live: false,
    },
    WorkflowStep {
        id: "desktop-visual",
        description: "Run native Desktop/Floating visual acceptance on Linux",
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
        id: "check-rust-sdk-surface",
        description: "Compile the standalone Rust SDK surface from the locked graph",
        action: WorkflowStepAction::Command(&[
            "cargo",
            "check",
            "--locked",
            "-p",
            "psychevo",
            "--no-default-features",
            "--all-targets",
        ]),
        live: false,
    },
    WorkflowStep {
        id: "verify-rust-sdk-packages",
        description: "Package and compile the three publishable Rust SDK crates",
        action: WorkflowStepAction::Command(&["sh", "scripts/verify-sdk-packages.sh"]),
        live: false,
    },
    PYTHON_SDK_TEST_STEP,
    PYTHON_PACKAGE_CONTRACTS_STEP,
    WorkflowStep {
        id: "build-cli-release",
        description: "Build release CLI artifact",
        action: WorkflowStepAction::ArtifactCommand {
            command: &[
                "cargo",
                "build",
                "--locked",
                "--release",
                "-p",
                "psychevo-cli",
                "--bin",
                "pevo",
            ],
            target_dir: "cli-target",
        },
        live: false,
    },
    WorkflowStep {
        id: "build-workbench",
        description: "Build Workbench artifact",
        action: WorkflowStepAction::Command(&["pnpm", "--filter", "@psychevo/workbench", "build"]),
        live: false,
    },
    WorkflowStep {
        id: "build-desktop-bundle",
        description: "Build the host Desktop bundle",
        action: WorkflowStepAction::ArtifactCommand {
            command: &["pnpm", "--filter", "@psychevo/desktop", "tauri:build"],
            target_dir: "desktop-target",
        },
        live: false,
    },
    NATIVE_ARTIFACT_SMOKE_STEP,
    INSTALLED_ARTIFACT_SMOKE_STEP,
    WorkflowStep {
        id: "checksum-local-artifacts",
        description: "Write local checksums without publishing artifacts",
        action: WorkflowStepAction::Command(&["python", "scripts/write_package_checksums.py"]),
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
        id: "rust-checks",
        description: "Rust workspace checks shard",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: RUST_CHECKS_STEPS,
    },
    WorkflowProfile {
        id: "rust-tests",
        description: "Rust workspace tests shard",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: RUST_TEST_STEPS,
    },
    WorkflowProfile {
        id: "sdk-contracts",
        description: "Rust, TypeScript, and Python SDK contract gate",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: SDK_CONTRACTS_STEPS,
    },
    WorkflowProfile {
        id: "main-artifact-smoke",
        description: "Build and exercise installable Linux SDK and App Server artifacts",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: MAIN_ARTIFACT_SMOKE_STEPS,
    },
    WorkflowProfile {
        id: "supply-chain",
        description: "Dependency, source, license, and committed-secret policy",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: SUPPLY_CHAIN_STEPS,
    },
    WorkflowProfile {
        id: "non-functional",
        description: "Linux performance, dependency, and artifact regression budgets",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: NON_FUNCTIONAL_STEPS,
    },
    WorkflowProfile {
        id: "instrumentation",
        description: "Scheduled coverage, deterministic boundaries, Miri, and sanitizer diagnostics",
        kind: ProfileKind::Ci,
        live: false,
        artifact_only: false,
        steps: INSTRUMENTATION_STEPS,
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
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde_yaml::Value as YamlValue;

    use super::*;

    #[test]
    fn profile_ids_are_unique() {
        let profiles = profile_summaries();
        let unique_ids = profiles
            .iter()
            .map(|profile| profile.id)
            .collect::<HashSet<_>>();
        assert_eq!(unique_ids.len(), profiles.len());
    }

    #[test]
    fn rust_broad_is_the_ordered_combination_of_hosted_shards() {
        let step_ids = |profile_id| {
            plan_profile(profile_id, None)
                .expect("profile")
                .steps
                .into_iter()
                .map(|step| step.id)
                .collect::<Vec<_>>()
        };
        let mut shard_ids = step_ids("rust-checks");
        shard_ids.extend(step_ids("rust-tests"));

        assert_eq!(step_ids("rust-broad"), shard_ids);
    }

    #[test]
    fn sdk_contracts_profile_owns_the_cross_language_contract() {
        let plan = plan_profile("sdk-contracts", None).expect("sdk-contracts profile");
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec![
                "sdk-architecture",
                "rust-sdk-default-surface",
                "gateway-protocol-check",
                "rust-app-server-contracts",
                "typescript-sdk-contracts",
                "test-python-sdk-client",
                "verify-python-package-contracts",
                "workflow-action-pins",
            ]
        );
        assert!(!plan.profile.live);
        assert!(!plan.profile.artifact_only);
    }

    #[test]
    fn main_artifact_smoke_reuses_only_the_installed_app_server_path() {
        let plan = plan_profile("main-artifact-smoke", None).expect("main artifact smoke");
        assert_eq!(plan.profile.kind, ProfileKind::Ci);
        assert!(!plan.profile.artifact_only);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].id, "smoke-installed-app-server-artifacts");
        assert_eq!(
            plan.steps[0].command,
            vec![
                "python",
                "python/tests/installed_artifact_smoke.py",
                "--app-server-only",
            ]
        );
    }

    #[test]
    fn supply_chain_profile_has_six_strict_non_live_steps() {
        let plan = plan_profile("supply-chain", None).expect("supply-chain profile");
        assert_eq!(plan.profile.kind, ProfileKind::Ci);
        assert!(!plan.profile.live);
        assert!(!plan.profile.artifact_only);
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec![
                "verify-supply-chain-tools",
                "rust-dependency-policy",
                "rust-unused-dependencies",
                "pnpm-production-advisories",
                "pnpm-dependency-hygiene",
                "committed-secret-scan",
            ]
        );
        assert_eq!(
            plan.steps[0].command,
            vec!["python", "scripts/supply_chain_tools.py", "verify"]
        );
        assert_eq!(
            plan.steps[1].command,
            vec![
                "cargo",
                "deny",
                "--locked",
                "check",
                "--config",
                "deny.toml",
                "advisories",
                "bans",
                "licenses",
                "sources",
            ]
        );
        assert_eq!(
            plan.steps[2].command,
            vec!["cargo-machete", "--with-metadata"]
        );
        assert_eq!(
            plan.steps[3].command,
            vec!["pnpm", "audit", "--prod", "--audit-level", "low"]
        );
        assert_eq!(
            plan.steps[4].command,
            vec!["pnpm", "run", "dependency:check"]
        );
        assert_eq!(
            plan.steps[5].command,
            vec![
                "gitleaks",
                "git",
                "--config",
                ".gitleaks.toml",
                "--redact",
                "--no-banner",
                "--log-opts=--all",
                ".",
            ]
        );
        assert!(plan.steps.iter().all(|step| !step.live));
        let forbidden = [
            "--ignore-registry-errors",
            "--ignore-unfixable",
            "--ignore",
            "--baseline-path",
        ];
        assert!(plan.steps.iter().all(|step| {
            forbidden
                .iter()
                .all(|flag| !step.command.iter().any(|part| part == flag))
        }));
    }

    #[test]
    fn non_functional_profile_owns_every_checked_budget_source() {
        let plan = plan_profile("non-functional", None).expect("non-functional profile");
        assert_eq!(plan.profile.kind, ProfileKind::Ci);
        assert!(!plan.profile.live);
        assert!(!plan.profile.artifact_only);
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec![
                "measure-framework-footprint",
                "build-budgeted-cli-release",
                "release-cli-startup-budget",
                "build-budgeted-workbench",
                "build-budgeted-desktop",
                "smoke-installed-python-artifacts",
                "gateway-first-result-budget",
                "gateway-idle-database-budget",
                "gateway-heartbeat-batch-budget",
                "gateway-retained-event-budget",
                "workbench-startup-budget",
                "enforce-linux-artifact-budgets",
            ]
        );
        assert!(plan.steps.iter().all(|step| !step.live));
        assert_eq!(
            plan.steps[0].command,
            vec!["python", "scripts/non_functional_budgets.py", "framework"]
        );
        assert_eq!(
            plan.steps[2].command,
            vec!["python", "scripts/non_functional_budgets.py", "cli-startup"]
        );
        assert_eq!(
            plan.steps[4].command,
            vec![
                "pnpm",
                "--filter",
                "@psychevo/desktop",
                "exec",
                "tauri",
                "build",
                "--features",
                "native-runtime",
                "--no-bundle",
            ]
        );
        assert_eq!(
            plan.steps[10].command,
            vec!["python", "scripts/non_functional_budgets.py", "workbench"]
        );
        assert_eq!(
            plan.steps[11].command,
            vec!["python", "scripts/non_functional_budgets.py", "artifacts"]
        );
    }

    #[test]
    fn instrumentation_profile_is_bounded_and_has_no_live_or_baseline_update_step() {
        let plan = plan_profile("instrumentation", None).expect("instrumentation profile");
        assert_eq!(plan.profile.kind, ProfileKind::Ci);
        assert!(!plan.profile.live);
        assert!(!plan.profile.artifact_only);
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec![
                "verify-instrumentation-harness",
                "verify-instrumentation-tools",
                "targeted-rust-coverage",
                "deterministic-boundary-contracts",
                "protocol-miri",
                "gateway-lifecycle-asan",
            ]
        );
        assert_eq!(
            plan.steps[0].command,
            vec![
                "python",
                "-m",
                "unittest",
                "discover",
                "-s",
                "scripts/tests",
                "-p",
                "test_high_risk_instrumentation.py",
                "-v",
            ]
        );
        for (step, command) in
            plan.steps
                .iter()
                .skip(1)
                .zip(["verify", "coverage", "deterministic", "miri", "asan"])
        {
            assert_eq!(
                step.command,
                vec!["python", "scripts/high_risk_instrumentation.py", command]
            );
            assert!(!step.live);
            assert!(!step.command.iter().any(|part| part.contains("update")));
        }
    }

    #[test]
    fn non_functional_and_instrumentation_manifests_are_explicit_and_monotonic() {
        let root = workspace_root();
        let budgets: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join("non-functional-budgets.json"))
                .expect("non-functional budgets"),
        )
        .expect("non-functional budget JSON");
        assert_eq!(budgets["schemaVersion"].as_u64(), Some(1));
        for scope in [
            "framework",
            "linuxArtifacts",
            "cliStartup",
            "workbench",
            "gateway",
        ] {
            let baseline = budgets[scope]["baseline"]
                .as_object()
                .unwrap_or_else(|| panic!("{scope} baseline"));
            let maximum = budgets[scope]["maximum"]
                .as_object()
                .unwrap_or_else(|| panic!("{scope} maximum"));
            assert_eq!(baseline.len(), maximum.len(), "{scope} metric count");
            for (name, value) in baseline {
                let maximum = maximum[name]
                    .as_u64()
                    .unwrap_or_else(|| panic!("{scope}.{name} maximum integer"));
                if let Some(baseline) = value.as_u64() {
                    assert!(maximum >= baseline, "{scope}.{name} budget regressed");
                } else {
                    assert!(value.is_null(), "{scope}.{name} baseline integer or null");
                }
            }
        }
        assert_eq!(
            budgets["gateway"]["maximum"]["idleSqliteOperations"].as_u64(),
            Some(0)
        );
        assert_eq!(
            budgets["gateway"]["maximum"]["shellHeartbeatTransactions"].as_u64(),
            Some(1)
        );
        for metric in [
            "retainedEventBatchCommitLatencyP50Ms",
            "retainedEventBatchCommitLatencyP95Ms",
            "retainedEventBatchCommitLatencyP99Ms",
            "retainedEventCommitLatencyP50Micros",
            "retainedEventCommitLatencyP95Micros",
            "retainedEventCommitLatencyP99Micros",
            "retainedEventMicrosPerEvent",
            "retainedEventPeakIngressQueueDepth",
            "retainedEventSqliteBusyOperations",
        ] {
            assert!(
                budgets["gateway"]["maximum"][metric].as_u64().is_some(),
                "missing retained-event maximum for {metric}"
            );
        }
        assert_eq!(
            budgets["gateway"]["maximum"]["retainedEventPeakIngressQueueDepth"].as_u64(),
            Some(32)
        );
        assert_eq!(
            budgets["gateway"]["maximum"]["retainedEventSqliteBusyOperations"].as_u64(),
            Some(0)
        );

        let tools = read_toml(&root.join("instrumentation-tools.toml"));
        assert_eq!(tools["schema"].as_integer(), Some(1));
        let nightly = tools["nightly"].as_str().expect("nightly pin");
        assert!(nightly.strip_prefix("nightly-").is_some_and(|date| {
            date.len() == 10
                && date.bytes().enumerate().all(|(index, byte)| {
                    matches!(index, 4 | 7) && byte == b'-'
                        || !matches!(index, 4 | 7) && byte.is_ascii_digit()
                })
        }));
        assert_eq!(tools["target"].as_str(), Some("x86_64-unknown-linux-gnu"));
        let version = tools["cargo-llvm-cov"]["version"]
            .as_str()
            .expect("coverage tool version");
        assert!(
            version
                .split('.')
                .all(|part| { !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) })
        );
    }

    #[test]
    fn hosted_instrumentation_is_weekly_manual_least_privilege_and_retained() {
        let root = workspace_root();
        let workflow: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(root.join(".github/workflows/instrumentation.yml"))
                .expect("instrumentation workflow"),
        )
        .expect("instrumentation workflow YAML");
        let triggers = yaml_field(&workflow, "on");
        assert!(yaml_field_optional(triggers, "schedule").is_some());
        assert!(yaml_field_optional(triggers, "workflow_dispatch").is_some());
        assert_eq!(
            yaml_field(yaml_field(&workflow, "permissions"), "contents").as_str(),
            Some("read")
        );
        let job = yaml_field(yaml_field(&workflow, "jobs"), "instrumentation");
        assert_eq!(yaml_field(job, "runs-on").as_str(), Some("ubuntu-24.04"));
        assert_eq!(yaml_field(job, "timeout-minutes").as_u64(), Some(180));
        let steps = yaml_field(job, "steps")
            .as_sequence()
            .expect("instrumentation steps");
        let checkout = workflow_step(steps, "Checkout repository");
        assert_eq!(
            yaml_field(yaml_field(checkout, "with"), "persist-credentials").as_bool(),
            Some(false)
        );
        let rust = workflow_step(steps, "Install stable Rust");
        assert!(yaml_field(rust, "uses").as_str().is_some());
        let run = workflow_step(steps, "Run bounded high-risk instrumentation");
        assert!(
            yaml_field(run, "run")
                .as_str()
                .is_some_and(|command| command.contains("--profile instrumentation"))
        );
        let upload = workflow_step(steps, "Retain structured instrumentation evidence");
        assert_eq!(yaml_field(upload, "if").as_str(), Some("${{ always() }}"));
        assert_eq!(
            yaml_field(yaml_field(upload, "with"), "if-no-files-found").as_str(),
            Some("error")
        );
    }

    #[test]
    fn hosted_ci_checks_out_before_git_based_scope_classification() {
        let root = workspace_root();
        let workflow: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("CI workflow"),
        )
        .expect("CI workflow YAML");
        let scope = yaml_field(yaml_field(&workflow, "jobs"), "scope");
        let steps = yaml_field(scope, "steps")
            .as_sequence()
            .expect("Scope steps");
        let checkout_index = steps
            .iter()
            .position(|step| {
                yaml_field_optional(step, "name").and_then(YamlValue::as_str)
                    == Some("Checkout repository")
            })
            .expect("Scope checkout");
        let classify_index = steps
            .iter()
            .position(|step| {
                yaml_field_optional(step, "name").and_then(YamlValue::as_str)
                    == Some("Classify pull request changes")
            })
            .expect("Scope classifier");
        assert!(checkout_index < classify_index);
        assert_eq!(
            yaml_field(
                yaml_field(&steps[checkout_index], "with"),
                "persist-credentials"
            )
            .as_bool(),
            Some(false)
        );
    }

    #[test]
    fn hosted_extended_validation_runs_visual_surface_and_budgets_serially() {
        let root = workspace_root();
        let source = fs::read_to_string(root.join(".github/workflows/extended-validation.yml"))
            .expect("extended validation workflow");
        assert!(!source.contains("secrets."));
        let workflow: YamlValue =
            serde_yaml::from_str(&source).expect("extended validation workflow YAML");
        let triggers = yaml_field(&workflow, "on");
        assert!(yaml_field_optional(triggers, "schedule").is_some());
        assert!(yaml_field_optional(triggers, "workflow_dispatch").is_some());
        assert_eq!(
            yaml_field(yaml_field(&workflow, "permissions"), "contents").as_str(),
            Some("read")
        );
        let job = yaml_field(yaml_field(&workflow, "jobs"), "extended");
        assert_eq!(yaml_field(job, "runs-on").as_str(), Some("ubuntu-24.04"));
        let steps = yaml_field(job, "steps")
            .as_sequence()
            .expect("extended validation steps");
        let checkout = workflow_step(steps, "Checkout repository");
        assert_eq!(
            yaml_field(yaml_field(checkout, "with"), "persist-credentials").as_bool(),
            Some(false)
        );
        let expected = [
            (
                "Run full deterministic visual inventory",
                "--profile visual",
            ),
            ("Run cross-surface profile", "--profile surface-profile"),
            ("Run non-functional budgets", "--profile non-functional"),
        ];
        let positions = expected.map(|(name, profile)| {
            let position = steps
                .iter()
                .position(|step| {
                    yaml_field_optional(step, "name").and_then(YamlValue::as_str) == Some(name)
                })
                .unwrap_or_else(|| panic!("missing workflow step {name}"));
            let command = yaml_field(&steps[position], "run")
                .as_str()
                .unwrap_or_else(|| panic!("{name} command"));
            assert!(command.contains(profile), "{name} must invoke {profile}");
            position
        });
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let upload = workflow_step(steps, "Retain extended validation evidence");
        assert_eq!(yaml_field(upload, "if").as_str(), Some("${{ always() }}"));
        assert_eq!(
            yaml_field(yaml_field(upload, "with"), "if-no-files-found").as_str(),
            Some("error")
        );
    }

    #[test]
    fn supply_chain_configs_pin_tools_without_blanket_exclusions() {
        let root = workspace_root();
        let tools = read_toml(&root.join("supply-chain-tools.toml"));
        assert_eq!(tools["schema"].as_integer(), Some(1));
        for name in ["cargo-deny", "cargo-machete", "gitleaks"] {
            let tool = tools[name].as_table().expect("tool table");
            let version = tool["version"].as_str().expect("tool version");
            assert!(version.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
            }));
            let url = tool["linux-x86_64-url"].as_str().expect("tool URL");
            assert!(url.starts_with("https://github.com/"));
            assert!(url.contains(version));
            assert!(!url.contains("latest"));
            let digest = tool["linux-x86_64-sha256"].as_str().expect("tool digest");
            assert_eq!(digest.len(), 64);
            assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }

        let package: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join("package.json")).expect("package.json"),
        )
        .expect("package JSON");
        let package_manager = package["packageManager"].as_str().expect("packageManager");
        let pnpm_version = package_manager
            .strip_prefix("pnpm@")
            .expect("exact pnpm package manager");
        assert!(
            pnpm_version
                .split('.')
                .all(|part| { !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) })
        );
        assert_eq!(package["devDependencies"]["knip"].as_str(), Some("6.31.0"));
        assert_eq!(
            package["scripts"]["dependency:check"].as_str(),
            Some("knip --dependencies --treat-config-hints-as-errors")
        );
        let knip: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join("knip.json")).expect("Knip configuration"),
        )
        .expect("Knip configuration JSON");
        assert!(knip["workspaces"].is_object());
        assert!(knip["ignoreBinaries"].is_array());

        let deny = read_toml(&root.join("deny.toml"));
        assert!(
            deny["advisories"]["ignore"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        assert_eq!(deny["advisories"]["yanked"].as_str(), Some("deny"));
        assert_eq!(deny["advisories"]["unmaintained"].as_str(), Some("all"));
        assert_eq!(deny["advisories"]["unsound"].as_str(), Some("all"));
        assert_eq!(deny["bans"]["multiple-versions"].as_str(), Some("allow"));
        assert_eq!(deny["bans"]["wildcards"].as_str(), Some("deny"));
        assert_eq!(deny["bans"]["allow-wildcard-paths"].as_bool(), Some(true));
        assert!(deny["bans"].get("skip").is_none());
        assert!(deny["bans"].get("skip-tree").is_none());
        assert_eq!(deny["sources"]["unknown-registry"].as_str(), Some("deny"));
        assert_eq!(deny["sources"]["unknown-git"].as_str(), Some("deny"));
        assert!(
            deny["sources"]["allow-git"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        assert!(
            !deny["licenses"]["allow"]
                .as_array()
                .expect("license allow list")
                .is_empty()
        );

        let xtask_manifest = read_toml(&root.join("xtask/Cargo.toml"));
        assert_eq!(xtask_manifest["package"]["publish"].as_bool(), Some(false));

        let gitleaks = read_toml(&root.join(".gitleaks.toml"));
        assert_eq!(gitleaks["extend"]["useDefault"].as_bool(), Some(true));
        assert!(gitleaks.get("allowlist").is_none());
        assert!(gitleaks.get("rules").is_none());
    }

    #[test]
    fn package_triggers_one_full_history_linux_supply_chain_job() {
        let root = workspace_root();
        let workflow: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(root.join(".github/workflows/package.yml"))
                .expect("package workflow"),
        )
        .expect("package workflow YAML");
        let triggers = yaml_field(&workflow, "on");
        for trigger in ["push", "release", "schedule", "workflow_dispatch"] {
            assert!(yaml_field_optional(triggers, trigger).is_some());
        }
        let tags = yaml_field(yaml_field(triggers, "push"), "tags")
            .as_sequence()
            .expect("tag triggers");
        assert_eq!(tags, &[YamlValue::String("v*".to_string())]);
        let release_types = yaml_field(yaml_field(triggers, "release"), "types")
            .as_sequence()
            .expect("release types");
        assert_eq!(release_types, &[YamlValue::String("published".to_string())]);

        let jobs = yaml_field(&workflow, "jobs");
        let supply_chain = yaml_field(jobs, "supply-chain");
        assert_eq!(
            yaml_field(supply_chain, "runs-on").as_str(),
            Some("ubuntu-24.04")
        );
        assert!(yaml_field_optional(supply_chain, "strategy").is_none());
        let steps = yaml_field(supply_chain, "steps")
            .as_sequence()
            .expect("supply-chain steps");
        let checkout = workflow_step(steps, "Checkout complete repository history");
        assert_eq!(
            yaml_field(yaml_field(checkout, "with"), "fetch-depth").as_u64(),
            Some(0)
        );
        let run = workflow_step(steps, "Run supply-chain policy");
        assert_eq!(
            yaml_field(run, "run").as_str(),
            Some("cargo xtask ci run --profile supply-chain")
        );
        let install = workflow_step(steps, "Install web dependencies");
        assert_eq!(
            yaml_field(install, "run").as_str(),
            Some("pnpm install --frozen-lockfile")
        );
        let install_index = steps
            .iter()
            .position(|step| std::ptr::eq(step, install))
            .expect("dependency install position");
        let run_index = steps
            .iter()
            .position(|step| std::ptr::eq(step, run))
            .expect("supply-chain position");
        assert!(install_index < run_index);
    }

    #[test]
    fn hosted_package_attests_exact_checksum_subjects_with_job_scoped_permissions() {
        let root = workspace_root();
        let workflow: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(root.join(".github/workflows/package.yml"))
                .expect("package workflow"),
        )
        .expect("package workflow YAML");
        let job = yaml_field(yaml_field(&workflow, "jobs"), "host-artifacts");
        let permissions = yaml_field(job, "permissions");
        assert_eq!(yaml_field(permissions, "contents").as_str(), Some("read"));
        assert_eq!(yaml_field(permissions, "id-token").as_str(), Some("write"));
        assert_eq!(
            yaml_field(permissions, "attestations").as_str(),
            Some("write")
        );
        assert_eq!(
            yaml_field(permissions, "artifact-metadata").as_str(),
            Some("write")
        );

        let steps = yaml_field(job, "steps")
            .as_sequence()
            .expect("host artifact steps");
        let package_index = steps
            .iter()
            .position(|step| {
                yaml_field_optional(step, "name").and_then(YamlValue::as_str)
                    == Some("Build, test, and smoke host artifacts")
            })
            .expect("package profile step");
        let attest_index = steps
            .iter()
            .position(|step| {
                yaml_field_optional(step, "name").and_then(YamlValue::as_str)
                    == Some("Attest package provenance")
            })
            .expect("provenance step");
        let upload_index = steps
            .iter()
            .position(|step| {
                yaml_field_optional(step, "name").and_then(YamlValue::as_str)
                    == Some("Retain reviewable host artifacts")
            })
            .expect("artifact upload step");
        assert!(package_index < attest_index && attest_index < upload_index);

        let attest = &steps[attest_index];
        assert_eq!(
            yaml_field(attest, "uses").as_str(),
            Some("actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6")
        );
        assert_eq!(
            yaml_field(yaml_field(attest, "with"), "subject-checksums").as_str(),
            Some(".local/package-matrix/${{ matrix.os }}/package/checksums.sha256")
        );

        let upload_path = yaml_field(yaml_field(&steps[upload_index], "with"), "path")
            .as_str()
            .expect("artifact upload path");
        assert!(upload_path.contains("${{ steps.provenance.outputs.bundle-path }}"));
        assert!(upload_path.contains("package/native-artifact-smoke.json"));
        assert!(upload_path.contains("package/native-artifact-smoke\n"));
        assert!(upload_path.contains("cli-target/release/pevo\n"));
        assert!(upload_path.contains("cli-target/release/pevo.exe"));
        assert!(upload_path.contains("desktop-target/release/psychevo-desktop\n"));
        assert!(upload_path.contains("desktop-target/release/psychevo-desktop.exe"));
        assert!(!upload_path.contains("pevo*"));
        assert!(!upload_path.contains("psychevo-desktop*"));

        let linux_dependencies = workflow_step(steps, "Install Linux Desktop build dependencies");
        let linux_install = yaml_field(linux_dependencies, "run")
            .as_str()
            .expect("Linux dependency install");
        for dependency in ["xauth", "xvfb"] {
            assert!(
                linux_install
                    .split_whitespace()
                    .any(|part| part == dependency),
                "package Linux dependencies omitted {dependency}"
            );
        }
    }

    #[test]
    fn workflow_action_references_are_full_commit_pins() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let mut invalid = Vec::new();
        for path in workflow_files(&root.join(".github/workflows")) {
            let source = fs::read_to_string(&path).expect("workflow source");
            for (line_index, line) in source.lines().enumerate() {
                let line = line.trim().strip_prefix("- ").unwrap_or(line.trim());
                let Some(reference) = line.strip_prefix("uses:") else {
                    continue;
                };
                let reference = reference.split_whitespace().next().expect("uses reference");
                if reference.starts_with("./") {
                    continue;
                }
                let valid = reference.rsplit_once('@').is_some_and(|(_, revision)| {
                    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
                if !valid {
                    invalid.push(format!(
                        "{}:{}: {reference}",
                        path.strip_prefix(root).unwrap_or(&path).display(),
                        line_index + 1
                    ));
                }
            }
        }
        assert!(
            invalid.is_empty(),
            "third-party workflow actions require exact 40-hex commit pins:\n{}",
            invalid.join("\n")
        );
    }

    fn workflow_files(directory: &Path) -> Vec<PathBuf> {
        let mut files = fs::read_dir(directory)
            .expect("workflow directory")
            .map(|entry| entry.expect("workflow entry").path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| matches!(extension, "yml" | "yaml"))
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    fn workspace_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
    }

    fn read_toml(path: &Path) -> toml::Value {
        toml::from_str(&fs::read_to_string(path).expect("TOML source")).expect("valid TOML")
    }

    fn yaml_field<'a>(value: &'a YamlValue, key: &str) -> &'a YamlValue {
        yaml_field_optional(value, key).unwrap_or_else(|| panic!("missing YAML field {key}"))
    }

    fn yaml_field_optional<'a>(value: &'a YamlValue, key: &str) -> Option<&'a YamlValue> {
        value
            .as_mapping()
            .expect("YAML mapping")
            .get(YamlValue::String(key.to_string()))
    }

    fn workflow_step<'a>(steps: &'a [YamlValue], name: &str) -> &'a YamlValue {
        steps
            .iter()
            .find(|step| yaml_field(step, "name").as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing workflow step {name}"))
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
            vec![
                "workspace-tests",
                "workspace-typecheck",
                "workspace-builds",
                "critical-browser-journey",
            ]
        );
        assert_eq!(
            plan.steps[0].command,
            vec!["pnpm", "--workspace-concurrency=1", "-r", "test"]
        );
        assert_eq!(
            plan.steps[3].command,
            vec!["xtask-internal", "critical-browser-journey"]
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
        assert_eq!(
            plan.steps.iter().map(|step| step.id).collect::<Vec<_>>(),
            vec![
                "check-rust-sdk-surface",
                "verify-rust-sdk-packages",
                "test-python-sdk-client",
                "verify-python-package-contracts",
                "build-cli-release",
                "build-workbench",
                "build-desktop-bundle",
                "smoke-native-release-artifacts",
                "smoke-installed-python-artifacts",
                "checksum-local-artifacts",
            ]
        );
        let command = |id| {
            plan.steps
                .iter()
                .find(|step| step.id == id)
                .expect("package step")
                .command
                .join(" ")
        };
        assert_eq!(
            command("build-cli-release"),
            "cargo build --locked --release -p psychevo-cli --bin pevo"
        );
        assert_eq!(
            command("build-desktop-bundle"),
            "pnpm --filter @psychevo/desktop tauri:build"
        );
        for id in [
            "verify-python-package-contracts",
            "smoke-native-release-artifacts",
            "smoke-installed-python-artifacts",
            "checksum-local-artifacts",
        ] {
            assert!(
                command(id).starts_with("python "),
                "{id} must use the cross-platform Python launcher"
            );
        }
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
        for step in plan
            .steps
            .into_iter()
            .filter(|step| step.command.first().is_some_and(|part| part == "cargo"))
        {
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
            vec![
                "desktop-manifest-parity",
                "desktop-format",
                "desktop-clippy",
                "desktop-tests"
            ]
        );

        let manifest = "apps/desktop/src-tauri/Cargo.toml";
        assert_eq!(
            plan.steps[1].command,
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
        for step in &plan.steps[2..] {
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
        assert!(plan.steps[2].command.ends_with(&[
            "--".to_string(),
            "-D".to_string(),
            "warnings".to_string()
        ]));
    }
}
