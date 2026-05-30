#[allow(unused_imports)]
use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalDiagnostic {
    pub schema_version: u32,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub severity: DiagnosticSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
}

impl EvalDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: VIEW_SCHEMA_VERSION,
            code: code.into(),
            message: message.into(),
            hint: None,
            severity: DiagnosticSeverity::Error,
            source: None,
        }
    }

    pub fn from_error(err: anyhow::Error) -> Self {
        let message = format!("{err:#}");
        if let Some(rest) = message.strip_prefix("incompatible_source_agent:") {
            return Self::error("incompatible_source_agent", rest.trim().to_string());
        }
        Self::error("peval_error", message)
    }
}

impl std::fmt::Display for EvalDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EvalDiagnostic {}

pub type ServiceResult<T> = std::result::Result<T, EvalDiagnostic>;

#[derive(Debug, Clone, Copy)]
pub enum ServiceCapability {
    Read,
    Write,
    Execute,
}

#[derive(Debug, Clone, Copy)]
pub struct ServiceCapabilities {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl ServiceCapabilities {
    pub fn all() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
        }
    }

    #[cfg(test)]
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
        }
    }

    pub fn allows(&self, capability: ServiceCapability) -> bool {
        match capability {
            ServiceCapability::Read => self.read,
            ServiceCapability::Write => self.write,
            ServiceCapability::Execute => self.execute,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceContext {
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub psychevo_home: Option<PathBuf>,
    pub root_override: Option<PathBuf>,
    pub capabilities: ServiceCapabilities,
}

impl ServiceContext {
    pub fn from_process() -> Result<Self> {
        Ok(Self {
            cwd: env::current_dir()?,
            env: inherited_env(),
            psychevo_home: None,
            root_override: None,
            capabilities: ServiceCapabilities::all(),
        })
    }

    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }

    pub fn effective_root(&self, request_root: Option<PathBuf>) -> Option<PathBuf> {
        request_root
            .or_else(|| self.root_override.clone())
            .map(|path| self.resolve_path(&path))
    }
}

#[derive(Debug, Clone)]
pub struct EvalService {
    context: ServiceContext,
}

impl EvalService {
    pub fn new(context: ServiceContext) -> Self {
        Self { context }
    }

    pub fn context(&self) -> &ServiceContext {
        &self.context
    }

    pub fn require(&self, capability: ServiceCapability) -> ServiceResult<()> {
        if self.context.capabilities.allows(capability) {
            Ok(())
        } else {
            Err(EvalDiagnostic::error(
                "capability_denied",
                format!("service context does not allow {capability:?} operations"),
            ))
        }
    }

    pub fn init(&self, request: InitStoreRequest) -> ServiceResult<InitStoreResult> {
        self.require(ServiceCapability::Write)?;
        let root = request
            .root
            .or_else(|| self.context.root_override.clone())
            .map(|path| self.context.resolve_path(&path));
        init_eval_store(InitStoreRequest {
            root,
            make_default: request.make_default,
            force: request.force,
        })
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn load_project(
        &self,
        config: Option<&Path>,
        benchmark: Option<&str>,
        store_root: Option<PathBuf>,
    ) -> ServiceResult<EvalProject> {
        self.require(ServiceCapability::Read)?;
        let resolved_config = config.map(|path| self.context.resolve_path(path));
        load_project_from_selection(
            resolved_config.as_deref(),
            benchmark,
            self.context.effective_root(store_root),
        )
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn try_load_project(
        &self,
        config: Option<&Path>,
        benchmark: Option<&str>,
        store_root: Option<PathBuf>,
    ) -> ServiceResult<Option<EvalProject>> {
        if config.is_some() || benchmark.is_some() {
            return self.load_project(config, benchmark, store_root).map(Some);
        }
        match discover_manifest(&self.context.cwd) {
            Ok(path) => EvalProject::load(path)
                .map(Some)
                .map_err(EvalDiagnostic::from_error),
            Err(_) => Ok(None),
        }
    }

    pub fn store(&self, store_root: Option<PathBuf>) -> ServiceResult<EvalStore> {
        self.require(ServiceCapability::Read)?;
        EvalStore::resolve(self.context.effective_root(store_root))
            .map_err(EvalDiagnostic::from_error)
    }

    pub fn list_datasets(&self, store_root: Option<PathBuf>) -> ServiceResult<Vec<DatasetEntry>> {
        self.require(ServiceCapability::Read)?;
        let store = EvalStore::resolve(self.context.effective_root(store_root))
            .map_err(EvalDiagnostic::from_error)?;
        store.list_datasets().map_err(EvalDiagnostic::from_error)
    }

    pub fn check(
        &self,
        project: &EvalProject,
        task_set: Option<&str>,
        task: Option<&str>,
        agent: Option<&str>,
    ) -> ServiceResult<Vec<CaseResult>> {
        self.require(ServiceCapability::Read)?;
        check_project(project, task_set, task, agent).map_err(EvalDiagnostic::from_error)
    }

    pub fn run(&self, request: RunRequest) -> ServiceResult<RunExecutionSummary> {
        self.require(ServiceCapability::Execute)?;
        self.require(ServiceCapability::Write)?;
        let config = request
            .config
            .map(|path| self.context.resolve_path(&path))
            .or_else(|| {
                request
                    .benchmark
                    .is_none()
                    .then(|| self.context.cwd.clone())
            });
        run_evaluation(RunRequest {
            config,
            benchmark: request.benchmark,
            task_set: request.task_set,
            task: request.task,
            agent: request.agent,
            overwrite: request.overwrite,
            store_root: self.context.effective_root(request.store_root),
            output_root: request
                .output_root
                .map(|path| self.context.resolve_path(&path)),
            include_artifacts: request.include_artifacts,
        })
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn create_task_env(
        &self,
        request: TaskEnvCreateRequest,
    ) -> ServiceResult<TaskEnvCreateResult> {
        self.require(ServiceCapability::Read)?;
        self.require(ServiceCapability::Write)?;
        let config = request
            .config
            .map(|path| self.context.resolve_path(&path))
            .or_else(|| {
                request
                    .benchmark
                    .is_none()
                    .then(|| self.context.cwd.clone())
            });
        create_task_env(TaskEnvCreateRequest {
            config,
            benchmark: request.benchmark,
            task_set: request.task_set,
            task: request.task,
            store_root: self.context.effective_root(request.store_root),
        })
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn verify_task_env(
        &self,
        request: TaskEnvVerifyRequest,
    ) -> ServiceResult<TaskEnvVerifyResult> {
        self.require(ServiceCapability::Execute)?;
        self.require(ServiceCapability::Write)?;
        verify_task_env(TaskEnvVerifyRequest {
            env_root: self.context.resolve_path(&request.env_root),
            duration_seconds: request.duration_seconds,
        })
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn view(&self, request: ViewRequest) -> ServiceResult<ViewReport> {
        self.require(ServiceCapability::Read)?;
        build_view(ViewRequest {
            config: request.config.map(|path| self.context.resolve_path(&path)),
            benchmark: request.benchmark,
            report: request.report,
            store_root: self.context.effective_root(request.store_root),
            paths: request.paths,
            task_set: request.task_set,
            agent: request.agent,
            task: request.task,
            status: request.status,
            group_by: request.group_by,
            include: request.include,
            notes: request.notes,
        })
        .map_err(EvalDiagnostic::from_error)
    }

    pub fn analysis_status(&self, request: &ViewRequest) -> ServiceResult<AnalysisStatus> {
        self.require(ServiceCapability::Read)?;
        analysis_status(self, request)
    }

    pub fn analyze_trial(&self, request: AnalysisTrialRequest) -> ServiceResult<AnalysisJson> {
        analyze_trial(self, request)
    }

    pub fn analyze_failed_batch(
        &self,
        request: AnalysisBatchRequest,
    ) -> ServiceResult<Vec<AnalysisJson>> {
        analyze_failed_batch(self, request)
    }

    pub fn dataset_import(&self, request: DatasetImportRequest) -> ServiceResult<DatasetEntry> {
        self.require(ServiceCapability::Write)?;
        import_dataset(DatasetImportRequest {
            store_root: self.context.effective_root(request.store_root),
            path: self.context.resolve_path(&request.path),
            ..request
        })
        .map_err(EvalDiagnostic::from_error)
    }
}
