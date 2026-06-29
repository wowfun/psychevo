#[allow(unused_imports)]
use super::*;

pub(crate) fn docker_compose_command(runtime: &ContainerRuntime, args: &[&str]) -> Result<Command> {
    let docker = find_program_on_path("docker").with_context(
        || "Docker CLI not found on PATH; install Docker Engine or Docker Desktop with Compose v2",
    )?;
    let mut command = Command::new(docker);
    command
        .arg("compose")
        .arg("--project-name")
        .arg(&runtime.project_name)
        .arg("-f")
        .arg(&runtime.compose_path);
    for arg in args {
        command.arg(arg);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    Ok(command)
}

pub(crate) fn docker_compose_exec_shell(
    runtime: &ContainerRuntime,
    cwd: &str,
    env_map: &BTreeMap<String, String>,
    shell: &str,
) -> Result<Command> {
    let mut command = docker_compose_command(runtime, &["exec", "-T"])?;
    command.arg("-w").arg(cwd);
    for (key, value) in env_map {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    command.arg("main").arg("sh").arg("-lc").arg(shell);
    Ok(command)
}

pub(crate) fn docker_compose_exec_process(
    runtime: &ContainerRuntime,
    cwd: &str,
    env_map: &BTreeMap<String, String>,
    program: &str,
    args: &[String],
) -> Result<Command> {
    let mut command = docker_compose_command(runtime, &["exec", "-T"])?;
    command.arg("-w").arg(cwd);
    for (key, value) in env_map {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    command.arg("main").arg(program);
    for arg in args {
        command.arg(arg);
    }
    Ok(command)
}

pub(crate) fn copy_into_container(
    runtime: &ContainerRuntime,
    source: &Path,
    target: &str,
) -> Result<()> {
    let parent = target
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/");
    let mkdir = docker_compose_exec_shell(
        runtime,
        "/",
        &BTreeMap::new(),
        &format!("mkdir -p {}", shell_quote(parent)),
    )?;
    let mkdir = wait_for_command(
        mkdir,
        Some(Duration::from_secs(30)),
        source.parent().unwrap_or(source),
    )?;
    if !mkdir.success {
        bail!(
            "failed to prepare container path `{target}`: {}",
            mkdir.stderr
        );
    }
    let mut command = docker_compose_command(runtime, &["cp"])?;
    command.arg(source).arg(format!("main:{target}"));
    let outcome = wait_for_command(
        command,
        Some(Duration::from_secs(120)),
        source.parent().unwrap_or(source),
    )?;
    if !outcome.success {
        bail!(
            "failed to copy {} into container {target}: {}",
            source.display(),
            outcome.stderr
        );
    }
    Ok(())
}

pub(crate) fn copy_from_container(
    runtime: &ContainerRuntime,
    source: &str,
    target: &Path,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut command = docker_compose_command(runtime, &["cp"])?;
    command.arg(format!("main:{source}")).arg(target);
    let outcome = wait_for_command(
        command,
        Some(Duration::from_secs(120)),
        target.parent().unwrap_or(target),
    )?;
    if !outcome.success {
        bail!(
            "failed to copy container path `{source}` to {}: {}",
            target.display(),
            outcome.stderr
        );
    }
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
