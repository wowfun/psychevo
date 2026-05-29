#[allow(unused_imports)]
use super::*;

pub fn run_cli_from<I, T>(args: I) -> CliOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => {
            let json_errors = command_wants_json(&cli.command);
            match dispatch_cli(cli) {
                Ok(outcome) => outcome,
                Err(err) if json_errors => {
                    let diagnostic = EvalDiagnostic::from_error(err);
                    CliOutcome {
                        code: 1,
                        stdout: String::new(),
                        stderr: format!(
                            "{}\n",
                            serde_json::to_string_pretty(&diagnostic)
                                .unwrap_or_else(|_| "{\"code\":\"peval_error\"}".to_string())
                        ),
                    }
                }
                Err(err) => CliOutcome {
                    code: 1,
                    stdout: String::new(),
                    stderr: format!("error: {err:#}\n"),
                },
            }
        }
        Err(err) => CliOutcome {
            code: if err.use_stderr() { 2 } else { 0 },
            stdout: if err.use_stderr() {
                String::new()
            } else {
                err.to_string()
            },
            stderr: if err.use_stderr() {
                err.to_string()
            } else {
                String::new()
            },
        },
    }
}

pub(crate) fn command_wants_json(command: &Commands) -> bool {
    match command {
        Commands::Init(args) => args.json,
        Commands::Project(ProjectCommands::Add(args)) => args.json,
        Commands::Project(ProjectCommands::List(args)) => args.json,
        Commands::Project(ProjectCommands::Remove(args)) => args.json,
        Commands::Doctor(args) => args.json,
        Commands::List(args) => args.json,
        Commands::Check(args) => args.json,
        Commands::Run(args) => args.json,
        Commands::Env(TaskEnvCommands::Create(args)) => args.json,
        Commands::Env(TaskEnvCommands::Verify(args)) => args.json,
        Commands::View(args) => effective_view_format(
            args.format,
            args.output.as_ref().and_then(|output| output.as_deref()),
            matches!(args.output, Some(None)),
        )
        .is_ok_and(|format| format == ViewFormat::Json),
        Commands::Serve(_) => false,
        Commands::Dataset(DatasetCommands::Import(args)) => args.json,
    }
}

pub(crate) fn dispatch_cli(cli: Cli) -> Result<CliOutcome> {
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Project(args) => run_project(args),
        Commands::Doctor(args) => run_doctor(args),
        Commands::List(args) => run_list(args),
        Commands::Check(args) => run_check(args),
        Commands::Run(args) => run_run(args),
        Commands::Env(args) => run_task_env(args),
        Commands::View(args) => run_view(args),
        Commands::Serve(args) => run_serve_command(args),
        Commands::Dataset(args) => run_dataset(args),
    }
}

pub(crate) fn process_service() -> Result<EvalService> {
    Ok(EvalService::new(ServiceContext::from_process()?))
}
