#[allow(unused_imports)]
use super::*;

pub(crate) fn harbor_container_environment(
    task: &TaskManifest,
) -> Result<HarborContainerEnvironment> {
    let task_toml = task.dir.join("task.toml");
    let raw = fs::read_to_string(&task_toml)
        .with_context(|| format!("failed to read {}", task_toml.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", task_toml.display()))?;
    let environment = value
        .get("environment")
        .and_then(toml::Value::as_table)
        .with_context(|| format!("task `{}` missing [environment]", task.id))?;
    let docker_image = environment
        .get("docker_image")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let allow_internet = environment
        .get("allow_internet")
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    let build_timeout_seconds = toml_value_as_u64(environment.get("build_timeout_sec"))
        .or_else(|| toml_value_as_u64(environment.get("build_timeout_seconds")))
        .unwrap_or(600);
    let cpus = toml_value_as_f64(environment.get("cpus"));
    let memory_mb = toml_value_as_u64(environment.get("memory_mb"));
    let cwd = environment
        .get("cwd")
        .and_then(toml::Value::as_str)
        .unwrap_or("/app")
        .to_string();
    if docker_image.is_none() && !task.dir.join("environment").join("Dockerfile").is_file() {
        bail!(
            "task `{}` declares neither environment.docker_image nor environment/Dockerfile",
            task.id
        );
    }
    Ok(HarborContainerEnvironment {
        docker_image,
        allow_internet,
        build_timeout_seconds,
        cpus,
        memory_mb,
        cwd,
    })
}

pub(crate) fn toml_value_as_u64(value: Option<&toml::Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_integer()
            .and_then(|value| u64::try_from(value).ok())
            .or_else(|| value.as_float().map(|value| value as u64))
    })
}

pub(crate) fn toml_value_as_f64(value: Option<&toml::Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_float()
            .or_else(|| value.as_integer().map(|value| value as f64))
    })
}
pub(crate) fn harbor_task_agent_timeout_seconds(task: &TaskManifest) -> Option<u64> {
    let task_toml = task.dir.join("task.toml");
    let raw = fs::read_to_string(&task_toml).ok()?;
    let value: toml::Value = toml::from_str(&raw).ok()?;
    value.get("agent").and_then(|agent| {
        toml_value_as_u64(agent.get("timeout_sec"))
            .or_else(|| toml_value_as_u64(agent.get("timeout_seconds")))
    })
}
