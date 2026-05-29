#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub struct CliOutcome {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Parser)]
#[command(name = "peval")]
#[command(about = "Run local Psychevo evaluation task sets")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Initialize a peval workspace")]
    Init(InitArgs),
    #[command(subcommand, hide = true, about = "Removed project registry commands")]
    Project(ProjectCommands),
    #[command(about = "Inspect local evaluation readiness")]
    Doctor(ProjectArgs),
    #[command(about = "List task sets, agents, tasks, view formats, and datasets")]
    List(ListArgs),
    #[command(about = "Validate evaluation manifests without executing cases")]
    Check(SelectArgs),
    #[command(about = "Run an evaluation matrix and write artifacts")]
    Run(RunArgs),
    #[command(
        subcommand,
        about = "Create and verify human-in-loop task environments"
    )]
    Env(TaskEnvCommands),
    #[command(about = "Render dynamic views over stored cell artifacts")]
    View(ViewArgs),
    #[command(about = "Serve the local peval workspace viewer")]
    Serve(ServeArgs),
    #[command(subcommand, about = "Manage local evaluation datasets")]
    Dataset(DatasetCommands),
}

#[derive(Debug, Parser)]
pub(crate) struct InitArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) root: Option<PathBuf>,
    #[arg(long = "default")]
    pub(crate) make_default: bool,
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommands {
    #[command(about = "Removed project registry command")]
    Add(ProjectAddArgs),
    #[command(about = "Removed project registry command")]
    List(ProjectListArgs),
    #[command(name = "rm", about = "Removed project registry command")]
    Remove(ProjectRemoveArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectAddArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: PathBuf,
    #[arg(long)]
    pub(crate) id: Option<String>,
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectListArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectRemoveArgs {
    pub(crate) id: String,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DatasetCommands {
    #[command(about = "Register a local dataset payload")]
    Import(DatasetImportArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ListArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ListKind::All)]
    pub(crate) kind: ListKind,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ListKind {
    All,
    TaskSets,
    Agents,
    Benchmarks,
    Tasks,
    Views,
    Datasets,
}

#[derive(Debug, Parser)]
pub(crate) struct SelectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long)]
    pub(crate) live: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct RunArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long)]
    pub(crate) overwrite: bool,
    #[arg(long, value_name = "DIR")]
    pub(crate) output_root: Option<PathBuf>,
    #[arg(long = "include", value_name = "ITEMS")]
    pub(crate) include: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskEnvCommands {
    #[command(about = "Create a human-editable task environment")]
    Create(TaskEnvCreateArgs),
    #[command(about = "Verify a human-edited task environment")]
    Verify(TaskEnvVerifyArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct TaskEnvCreateArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct TaskEnvVerifyArgs {
    #[arg(long = "env", value_name = "DIR")]
    pub(crate) env_root: PathBuf,
    #[arg(long = "duration-seconds", value_name = "N")]
    pub(crate) duration_seconds: u64,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ViewArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(long = "report", value_name = "KEY")]
    pub(crate) report: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<CaseStatusFilter>,
    #[arg(long = "group-by", value_name = "ITEMS")]
    pub(crate) group_by: Vec<String>,
    #[arg(short = 'i', long = "include", value_name = "ITEMS")]
    pub(crate) include: Vec<String>,
    #[arg(long, value_enum)]
    pub(crate) format: Option<ViewFormat>,
    #[arg(short = 'o', long, value_name = "PATH", num_args = 0..=1)]
    pub(crate) output: Option<Option<PathBuf>>,
}

#[derive(Debug, Parser)]
pub(crate) struct ServeArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(long = "report", value_name = "KEY")]
    pub(crate) report: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<CaseStatusFilter>,
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: std::net::IpAddr,
    #[arg(long, default_value_t = 0)]
    pub(crate) port: u16,
}

#[derive(Debug, Parser)]
pub(crate) struct DatasetImportArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) id: Option<String>,
    #[arg(long)]
    pub(crate) name: Option<String>,
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long)]
    pub(crate) loader: Option<String>,
    #[arg(long)]
    pub(crate) split: Option<String>,
    #[arg(long)]
    pub(crate) sample_limit: Option<usize>,
    #[arg(long)]
    pub(crate) cache_key: Option<String>,
    #[arg(long)]
    pub(crate) license: Option<String>,
    #[arg(long = "tag")]
    pub(crate) tags: Vec<String>,
    #[arg(long)]
    pub(crate) notes: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}
