#[allow(unused_imports)]
use crate::*;

#[derive(Debug, Clone)]
pub struct EvalStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitStoreResult {
    pub schema_version: u32,
    pub root: PathBuf,
    pub default_workspace: bool,
}
impl EvalProject {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_manifest(start.as_ref())?;
        load_eval_config(&manifest_path, None)
    }

    pub fn slug(&self) -> String {
        slugify(&self.benchmark_id)
    }
}

impl BenchmarkManifest {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_benchmark_manifest(start.as_ref())?;
        let root = manifest_path
            .parent()
            .context("benchmark.toml has no parent directory")?
            .to_path_buf();
        let manifest_raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let version: ManifestVersion = toml::from_str(&manifest_raw).with_context(|| {
            format!(
                "failed to parse schema_version in {}",
                manifest_path.display()
            )
        })?;
        reject_unsupported(version.schema_version, &manifest_path)?;
        let raw: RawBenchmarkManifestSerde = toml::from_str(&manifest_raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        let (sources, task_sets, tasks) =
            load_benchmark_sources(&root, &manifest_path, raw.sources)?;
        Ok(BenchmarkManifest {
            root,
            manifest_path,
            schema_version: raw.schema_version,
            id: slugify(&raw.id),
            name: raw.name.unwrap_or(raw.id),
            sources,
            task_sets,
            tasks,
        })
    }
}

impl EvalStore {
    pub fn resolve(store_root: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            root: resolve_store_root(store_root)?,
        })
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn cell_runs_root(&self, project: &EvalProject) -> PathBuf {
        self.root.join("runs").join(project.slug())
    }

    pub fn cell_root(&self, project: &EvalProject, case: &CasePlan, cell_key: &str) -> PathBuf {
        self.cell_runs_root(project)
            .join(sanitize_id(&case.agent.id))
            .join(sanitize_id(&case.task.id))
            .join(cell_key)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("runs"))
            .with_context(|| format!("failed to create {}", self.root.join("runs").display()))?;
        fs::create_dir_all(self.root.join("datasets")).with_context(|| {
            format!("failed to create {}", self.root.join("datasets").display())
        })?;
        fs::create_dir_all(self.root.join("scripts"))
            .with_context(|| format!("failed to create {}", self.root.join("scripts").display()))?;
        Ok(())
    }

    pub fn list_datasets(&self) -> Result<Vec<DatasetEntry>> {
        let mut entries = Vec::new();
        let datasets_dir = self.root.join("datasets");
        if !datasets_dir.is_dir() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&datasets_dir)
            .with_context(|| format!("failed to read {}", datasets_dir.display()))?
        {
            let path = entry?.path().join("dataset.toml");
            if path.is_file() {
                entries.push(read_dataset_entry(&path)?);
            }
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(entries)
    }

    pub fn refresh_after_dataset_change(&self) -> Result<()> {
        Ok(())
    }

    pub fn scan_cell_runs(&self, scope: &Path) -> Result<Vec<CellRun>> {
        let scope = if scope.is_absolute() {
            scope.to_path_buf()
        } else {
            self.root.join(scope)
        };
        let mut cells = Vec::new();
        if !scope.is_dir() {
            return Ok(cells);
        }
        self.scan_cell_runs_in(&scope, &mut cells)?;
        cells.sort_by(|left, right| {
            left.benchmark
                .cmp(&right.benchmark)
                .then_with(|| left.case.agent_id.cmp(&right.case.agent_id))
                .then_with(|| left.case.task_id.cmp(&right.case.task_id))
                .then_with(|| left.cell_key.cmp(&right.cell_key))
        });
        Ok(cells)
    }

    fn scan_cell_runs_in(&self, dir: &Path, cells: &mut Vec<CellRun>) -> Result<()> {
        let run_json = dir.join("run.json");
        if run_json.is_file() {
            if let Ok(cell) = read_cell_run(dir) {
                cells.push(cell);
            }
            return Ok(());
        }
        for entry in
            fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_cell_runs_in(&path, cells)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn workspace_config_path(root: &Path) -> PathBuf {
    root.join("peval.toml")
}

pub(crate) fn global_peval_config_path(home: &Path) -> PathBuf {
    home.join("peval-config.toml")
}

pub(crate) fn ensure_workspace_config(root: &Path) -> Result<()> {
    let path = workspace_config_path(root);
    if path.is_file() {
        let _ = read_workspace_config(root)?;
        return Ok(());
    }
    let config = PevalWorkspaceConfig::default();
    write_toml_pretty(&path, &config)
}

pub(crate) fn read_workspace_config(root: &Path) -> Result<PevalWorkspaceConfig> {
    let path = workspace_config_path(root);
    let config: PevalWorkspaceConfig = read_toml(&path)?;
    reject_unsupported_workspace(config.schema_version, &path)?;
    if config.kind != "workspace" {
        bail!(
            "{} is not a peval workspace config; expected kind = \"workspace\"",
            path.display()
        );
    }
    Ok(config)
}

pub(crate) fn read_global_peval_config(home: &Path) -> Result<PevalGlobalConfig> {
    let path = global_peval_config_path(home);
    if !path.is_file() {
        return Ok(PevalGlobalConfig::default());
    }
    let config: PevalGlobalConfig = read_toml(&path)?;
    reject_unsupported_workspace(config.schema_version, &path)?;
    Ok(config)
}

pub(crate) fn write_global_peval_config(home: &Path, config: &PevalGlobalConfig) -> Result<()> {
    fs::create_dir_all(home).with_context(|| format!("failed to create {}", home.display()))?;
    write_toml_pretty(&global_peval_config_path(home), config)
}

pub(crate) fn write_default_workspace(home: &Path, root: &Path, force: bool) -> Result<()> {
    let mut config = read_global_peval_config(home)?;
    if let Some(existing) = &config.default_workspace
        && existing != root
        && !force
    {
        bail!(
            "{} already points to {}; rerun `peval init --default --force --root {}` to replace it",
            global_peval_config_path(home).display(),
            existing.display(),
            root.display()
        );
    }
    config.default_workspace = Some(root.to_path_buf());
    write_global_peval_config(home, &config)
}

pub(crate) const DEFAULT_EVAL_TEMPLATES: &[(&str, &str)] = &[(
    "pidx-psychevo-acp.eval.toml",
    include_str!("../templates/pidx-psychevo-acp.eval.toml"),
)];

pub(crate) fn default_eval_template_paths(root: &Path) -> Vec<PathBuf> {
    DEFAULT_EVAL_TEMPLATES
        .iter()
        .map(|(name, _)| root.join(name))
        .collect()
}

pub(crate) fn copy_workspace_templates(root: &Path) -> Result<()> {
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts)
        .with_context(|| format!("failed to create {}", scripts.display()))?;
    for (name, content) in DEFAULT_EVAL_TEMPLATES {
        let target = root.join(name);
        if target.exists() {
            continue;
        }
        fs::write(&target, content)
            .with_context(|| format!("failed to write {}", target.display()))?;
    }
    Ok(())
}

pub fn init_eval_store(request: InitStoreRequest) -> Result<InitStoreResult> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let root = if let Some(path) = request.root {
        resolve_explicit_path(&path, &env_map, &cwd)?
    } else {
        cwd
    };
    fs::create_dir_all(&root).with_context(|| format!("failed to create {}", root.display()))?;
    let root = absolute_path(&root);
    ensure_workspace_config(&root)?;
    copy_workspace_templates(&root)?;
    EvalStore::new(root.clone()).ensure_layout()?;

    let mut default_workspace = false;
    if request.make_default {
        write_default_workspace(&home, &root, request.force)?;
        default_workspace = true;
    }

    Ok(InitStoreResult {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        root,
        default_workspace,
    })
}

pub(crate) fn check_project(
    project: &EvalProject,
    task_set_filter: Option<&str>,
    task_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CaseResult>> {
    let cases = expand_matrix(project, task_set_filter, task_filter, agent_filter)?;
    for case in &cases {
        validate_case(case)?;
    }
    Ok(cases
        .into_iter()
        .map(|case| CaseResult {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            identity: CaseIdentity {
                case_id: case.case_id.clone(),
                task_set_id: case.task_set.id.clone(),
                task_id: case.task.id.clone(),
                task_family: case.task.kind.clone(),
            },
            candidate: CandidateIdentity {
                agent_id: case.agent.id.clone(),
                adapter: case.agent.kind,
                model: agent_model(&case.agent),
            },
            factors: CaseFactors::default(),
            case_id: case.case_id,
            task_set_id: case.task_set.id,
            task_id: case.task.id,
            task_family: case.task.kind,
            agent_id: case.agent.id,
            status: CaseStatus::Passed,
            failure_class: None,
            score: ScoreResult {
                schema_version: EVALUATOR_RESULT_SCHEMA_VERSION,
                passed: true,
                score: None,
                message: "validated".to_string(),
                details: Value::Null,
            },
            duration_ms: 0,
            metrics: CaseMetrics::default(),
            warnings: Vec::new(),
            artifacts: CaseArtifacts {
                result: PathBuf::new(),
                trajectory: PathBuf::new(),
                evaluator_stdout: PathBuf::new(),
                evaluator_stderr: PathBuf::new(),
            },
        })
        .collect())
}
