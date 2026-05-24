use std::process::ExitCode;

#[tokio::main]
pub(crate) async fn main() -> ExitCode {
    match psychevo_acp::run_stdio(psychevo_acp::AcpOptions::from_env()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}
