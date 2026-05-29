use std::process::ExitCode;

#[tokio::main]
pub(crate) async fn main() -> ExitCode {
    if std::env::args().any(|arg| arg == "--setup") {
        println!(
            "Run `pevo auth setup --provider <id> --model <model> --base-url <url> --api-key-stdin` or add `--no-auth` for explicit no-auth providers."
        );
        return ExitCode::SUCCESS;
    }
    match psychevo_acp::run_stdio(psychevo_acp::AcpOptions::from_env()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}
