#[allow(unused_imports)]
use super::*;

pub(crate) fn prepare_harbor_compose(
    case: &CasePlan,
    artifact_root: &Path,
    logs_dir: &Path,
    environment: &HarborContainerEnvironment,
) -> Result<ContainerRuntime> {
    let compose_dir = artifact_root.join("container");
    fs::create_dir_all(&compose_dir)
        .with_context(|| format!("failed to create {}", compose_dir.display()))?;
    fs::create_dir_all(artifact_root.join("agent-state")).with_context(|| {
        format!(
            "failed to create {}",
            artifact_root.join("agent-state").display()
        )
    })?;
    let project_name = format!(
        "peval-{}",
        stable_hash_hex(&format!("{}:{}", case.case_id, now_ms()))
            .chars()
            .take(12)
            .collect::<String>()
    );
    let compose_path = compose_dir.join("docker-compose.yml");
    let artifact_root = fs::canonicalize(artifact_root)
        .with_context(|| format!("failed to resolve {}", artifact_root.display()))?;
    let logs_dir = fs::canonicalize(logs_dir)
        .with_context(|| format!("failed to resolve {}", logs_dir.display()))?;
    let environment_dir =
        fs::canonicalize(case.task.dir.join("environment")).with_context(|| {
            format!(
                "failed to resolve {}",
                case.task.dir.join("environment").display()
            )
        })?;
    let mut yaml = String::new();
    yaml.push_str("services:\n");
    yaml.push_str("  main:\n");
    if let Some(image) = &environment.docker_image {
        yaml.push_str(&format!("    image: {}\n", yaml_scalar(image)));
        yaml.push_str("    pull_policy: if_not_present\n");
    } else {
        yaml.push_str("    build:\n");
        yaml.push_str(&format!(
            "      context: {}\n",
            yaml_scalar(&environment_dir.display().to_string())
        ));
    }
    yaml.push_str("    command: [\"sh\", \"-lc\", \"sleep infinity\"]\n");
    yaml.push_str(&format!(
        "    working_dir: {}\n",
        yaml_scalar(&environment.workdir)
    ));
    if !environment.allow_internet {
        yaml.push_str("    network_mode: none\n");
    }
    if let Some(cpus) = environment.cpus {
        yaml.push_str(&format!("    cpus: {}\n", yaml_scalar(&cpus.to_string())));
    }
    if let Some(memory_mb) = environment.memory_mb {
        yaml.push_str(&format!(
            "    mem_limit: {}\n",
            yaml_scalar(&format!("{memory_mb}m"))
        ));
    }
    yaml.push_str("    volumes:\n");
    yaml.push_str("      - type: bind\n");
    yaml.push_str(&format!(
        "        source: {}\n",
        yaml_scalar(&logs_dir.display().to_string())
    ));
    yaml.push_str("        target: /logs\n");
    yaml.push_str("      - type: bind\n");
    yaml.push_str(&format!(
        "        source: {}\n",
        yaml_scalar(&artifact_root.display().to_string())
    ));
    yaml.push_str("        target: /peval\n");
    fs::write(&compose_path, yaml)
        .with_context(|| format!("failed to write {}", compose_path.display()))?;
    Ok(ContainerRuntime {
        project_name,
        compose_path,
        workdir: environment.workdir.clone(),
    })
}

pub(crate) fn yaml_scalar(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
