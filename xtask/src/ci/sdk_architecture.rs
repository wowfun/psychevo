use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use toml::Value;

const REQUIRED_CRATES: &[&str] = &[
    "psychevo-ai",
    "psychevo-agent-core",
    "psychevo",
    "psychevo-gateway-protocol",
    "psychevo-gateway",
    "psychevo-acp",
    "psychevo-cli",
];

const REQUIRED_DEPENDENCIES: &[(&str, &[&str])] = &[
    ("psychevo-agent-core", &["psychevo-ai"]),
    ("psychevo", &["psychevo-agent-core", "psychevo-ai"]),
    (
        "psychevo-gateway",
        &["psychevo", "psychevo-gateway-protocol"],
    ),
    ("psychevo-acp", &["psychevo"]),
    (
        "psychevo-cli",
        &["psychevo", "psychevo-acp", "psychevo-gateway"],
    ),
];

const LAYERS: &[(&str, usize)] = &[
    ("psychevo-ai", 0),
    ("psychevo-gateway-protocol", 0),
    ("psychevo-agent-core", 1),
    ("psychevo", 2),
    ("psychevo-gateway", 3),
    ("psychevo-acp", 3),
    ("psychevo-cli", 4),
];

pub(crate) fn check_sdk_architecture(root: &Path) -> Result<()> {
    let workspace = manifest(&root.join("Cargo.toml"))?;
    let member_paths = string_array(value_at(&workspace, &["workspace", "members"])?)?;
    let mut manifests = BTreeMap::new();
    for member_path in member_paths {
        let member = manifest(&root.join(&member_path).join("Cargo.toml"))?;
        let name = value_at(&member, &["package", "name"])?
            .as_str()
            .context("workspace package.name must be a string")?
            .to_string();
        if manifests.insert(name.clone(), member).is_some() {
            bail!("workspace contains duplicate package name {name}");
        }
    }

    for required in REQUIRED_CRATES {
        if !manifests.contains_key(*required) {
            bail!("workspace is missing required product crate {required}");
        }
    }

    let product_manifests = manifests
        .iter()
        .filter(|(name, _)| name.as_str() != "psychevo-xtask")
        .map(|(name, manifest)| (name.clone(), manifest))
        .collect::<BTreeMap<_, _>>();
    let product_names = product_manifests.keys().cloned().collect::<BTreeSet<_>>();
    let graph = product_manifests
        .iter()
        .map(|(name, manifest)| {
            (
                name.clone(),
                workspace_dependencies(manifest, &product_names),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_dependency_topology(&graph)?;

    for (owner, required) in REQUIRED_DEPENDENCIES {
        let actual = graph
            .get(*owner)
            .with_context(|| format!("missing dependency graph node {owner}"))?;
        for dependency in *required {
            if !actual.contains(*dependency) {
                bail!("{owner} must depend on {dependency}");
            }
        }
    }

    let expected_version = value_at(&manifests["psychevo"], &["package", "version"])?
        .as_str()
        .context("psychevo package.version must be a string")?;
    for name in REQUIRED_CRATES {
        let product = &manifests[*name];
        if value_at(product, &["package", "version"])?.as_str() != Some(expected_version) {
            bail!("{name} must use the shared SDK product version {expected_version}");
        }
    }
    for (name, product) in &product_manifests {
        for forbidden in ["opentelemetry", "tracing-opentelemetry"] {
            if dependency_table(product).contains_key(forbidden) {
                bail!("{name} must not depend on outbound telemetry crate {forbidden}");
            }
        }
    }

    for published in ["psychevo-ai", "psychevo-agent-core", "psychevo"] {
        let product = &manifests[published];
        if value_at_optional(product, &["package", "publish"]).is_some() {
            bail!("{published} must remain publishable");
        }
        for dependency in dependency_table(product)
            .iter()
            .filter(|(name, value)| {
                let package = dependency_package_name(name, value);
                product_names.contains(package)
            })
            .map(|(_, value)| value)
        {
            let table = dependency
                .as_table()
                .context("published product dependency must be a table")?;
            if table.get("path").and_then(Value::as_str).is_none()
                || table.get("version").and_then(Value::as_str).is_none()
            {
                bail!(
                    "{published} product dependencies need both path and released version declarations"
                );
            }
        }
    }
    for private in [
        "psychevo-gateway-protocol",
        "psychevo-gateway",
        "psychevo-acp",
        "psychevo-cli",
    ] {
        if value_at(&manifests[private], &["package", "publish"])?.as_bool() != Some(false) {
            bail!("{private} must set publish = false");
        }
    }

    let framework_features = value_at(&manifests["psychevo"], &["features"])?
        .as_table()
        .context("psychevo features must be a table")?;
    if !framework_features
        .get("default")
        .is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
        || !framework_features.contains_key("product")
    {
        bail!("psychevo must have an empty default feature set and a product bridge feature");
    }
    for private in ["psychevo-gateway", "psychevo-acp", "psychevo-cli"] {
        let dependency = dependency_table(&manifests[private])
            .get("psychevo")
            .and_then(Value::as_table)
            .with_context(|| format!("{private} must depend on psychevo through a table"))?;
        let features = dependency
            .get("features")
            .map(string_array)
            .transpose()?
            .unwrap_or_default();
        if !features.contains("product") {
            bail!("{private} must opt into psychevo's product bridge feature");
        }
    }
    for python_project in [
        "python/psychevo/pyproject.toml",
        "python/app-server-bin/pyproject.toml",
        "python/cli-bin/pyproject.toml",
    ] {
        let project = manifest(&root.join(python_project))?;
        if value_at(&project, &["project", "version"])?.as_str() != Some(expected_version) {
            bail!("{python_project} must use the shared SDK product version {expected_version}");
        }
    }
    Ok(())
}

fn validate_dependency_topology(graph: &BTreeMap<String, BTreeSet<String>>) -> Result<()> {
    for (owner, dependencies) in graph {
        for dependency in dependencies {
            if !graph.contains_key(dependency) {
                bail!("{owner} depends on unknown workspace crate {dependency}");
            }
        }
    }
    if let Some(cycle) = find_cycle(graph) {
        bail!("workspace product dependency cycle: {}", cycle.join(" -> "));
    }

    let layers = LAYERS.iter().copied().collect::<BTreeMap<_, _>>();
    for (owner, owner_layer) in &layers {
        let reachable = reachable_dependencies(graph, owner);
        for dependency in reachable {
            let Some(dependency_layer) = layers.get(dependency.as_str()) else {
                continue;
            };
            if dependency_layer > owner_layer {
                bail!(
                    "{owner} at architecture layer {owner_layer} depends on higher-layer {dependency} at {dependency_layer}"
                );
            }
        }
    }
    Ok(())
}

fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut BTreeSet<String>,
        active: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if let Some(position) = active.iter().position(|entry| entry == node) {
            let mut cycle = active[position..].to_vec();
            cycle.push(node.to_string());
            return Some(cycle);
        }
        if !visited.insert(node.to_string()) {
            return None;
        }
        active.push(node.to_string());
        for dependency in &graph[node] {
            if let Some(cycle) = visit(dependency, graph, visited, active) {
                return Some(cycle);
            }
        }
        active.pop();
        None
    }

    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        if let Some(cycle) = visit(node, graph, &mut visited, &mut Vec::new()) {
            return Some(cycle);
        }
    }
    None
}

fn reachable_dependencies(
    graph: &BTreeMap<String, BTreeSet<String>>,
    start: &str,
) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let mut pending = graph
        .get(start)
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    while let Some(next) = pending.pop() {
        if !reachable.insert(next.clone()) {
            continue;
        }
        pending.extend(graph.get(&next).into_iter().flatten().cloned());
    }
    reachable
}

fn workspace_dependencies(
    manifest: &Value,
    workspace_names: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    collect_dependency_section(
        manifest.get("dependencies"),
        workspace_names,
        &mut dependencies,
    );
    collect_dependency_section(
        manifest.get("build-dependencies"),
        workspace_names,
        &mut dependencies,
    );
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            collect_dependency_section(
                target.get("dependencies"),
                workspace_names,
                &mut dependencies,
            );
            collect_dependency_section(
                target.get("build-dependencies"),
                workspace_names,
                &mut dependencies,
            );
        }
    }
    dependencies
}

fn collect_dependency_section(
    section: Option<&Value>,
    workspace_names: &BTreeSet<String>,
    dependencies: &mut BTreeSet<String>,
) {
    let Some(section) = section.and_then(Value::as_table) else {
        return;
    };
    for (name, value) in section {
        let package = dependency_package_name(name, value);
        if workspace_names.contains(package) {
            dependencies.insert(package.to_string());
        }
    }
}

fn dependency_package_name<'a>(name: &'a str, value: &'a Value) -> &'a str {
    value
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(Value::as_str)
        .unwrap_or(name)
}

fn manifest(path: &Path) -> Result<Value> {
    fs::read_to_string(path)
        .with_context(|| format!("read Cargo manifest {}", path.display()))?
        .parse()
        .with_context(|| format!("parse Cargo manifest {}", path.display()))
}

fn dependency_table(value: &Value) -> &toml::map::Map<String, Value> {
    value
        .get("dependencies")
        .and_then(Value::as_table)
        .expect("checked Cargo manifests have dependency tables")
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    value_at_optional(value, path)
        .with_context(|| format!("missing Cargo manifest field {}", path.join(".")))
}

fn value_at_optional<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, segment| current.get(*segment))
}

fn string_array(value: &Value) -> Result<BTreeSet<String>> {
    value
        .as_array()
        .context("Cargo manifest field must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .context("Cargo manifest array entry must be a string")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        edges
            .iter()
            .map(|(owner, dependencies)| {
                (
                    (*owner).to_string(),
                    dependencies
                        .iter()
                        .map(|dependency| (*dependency).to_string())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn current_workspace_has_the_sdk_architecture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        check_sdk_architecture(root).expect("SDK architecture");
    }

    #[test]
    fn topology_accepts_an_uninventoried_internal_implementation_crate() {
        let graph = graph(&[
            ("psychevo-ai", &[]),
            ("psychevo-gateway-protocol", &[]),
            ("psychevo-agent-core", &["psychevo-ai"]),
            ("runtime-implementation", &["psychevo-agent-core"]),
            ("psychevo", &["runtime-implementation"]),
            ("psychevo-gateway", &["psychevo"]),
            ("psychevo-acp", &["psychevo"]),
            ("psychevo-cli", &["psychevo-gateway", "psychevo-acp"]),
        ]);

        validate_dependency_topology(&graph).expect("valid inserted implementation layer");
    }

    #[test]
    fn topology_rejects_a_transitive_reverse_dependency() {
        let graph = graph(&[
            ("psychevo-ai", &["private-helper"]),
            ("psychevo-gateway-protocol", &[]),
            ("private-helper", &["psychevo-gateway"]),
            ("psychevo-agent-core", &["psychevo-ai"]),
            ("psychevo", &["psychevo-agent-core"]),
            ("psychevo-gateway", &[]),
            ("psychevo-acp", &["psychevo"]),
            ("psychevo-cli", &["psychevo-gateway", "psychevo-acp"]),
        ]);

        let error = validate_dependency_topology(&graph).expect_err("reverse dependency");
        assert!(error.to_string().contains("higher-layer psychevo-gateway"));
    }

    #[test]
    fn topology_rejects_cycles_through_internal_crates() {
        let graph = graph(&[
            ("psychevo-ai", &["private-helper"]),
            ("private-helper", &["psychevo-ai"]),
        ]);

        let error = validate_dependency_topology(&graph).expect_err("cycle");
        assert!(error.to_string().contains("dependency cycle"));
    }
}
