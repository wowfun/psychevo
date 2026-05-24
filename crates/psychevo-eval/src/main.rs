use std::process::ExitCode;

pub(crate) fn main() -> ExitCode {
    let outcome = psychevo_eval::run_cli_from(std::env::args_os());
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        eprint!("{}", outcome.stderr);
    }
    ExitCode::from(outcome.code)
}
