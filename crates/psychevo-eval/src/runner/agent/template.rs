#[allow(unused_imports)]
use super::*;

pub(crate) fn render_agent_template(
    value: &str,
    workspace: &Path,
    task_dir: &Path,
    prompt: &str,
    prompt_file: &Path,
) -> String {
    value
        .replace("{workspace}", &workspace.display().to_string())
        .replace("{task_dir}", &task_dir.display().to_string())
        .replace("{prompt_file}", &prompt_file.display().to_string())
        .replace("{prompt}", prompt)
}
