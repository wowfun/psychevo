use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use toml::Value;

const PRODUCT_CRATES: &[(&str, &str)] = &[
    ("psychevo-ai", "crates/psychevo-ai"),
    ("psychevo-agent-core", "crates/psychevo-agent-core"),
    ("psychevo", "crates/psychevo"),
    (
        "psychevo-gateway-protocol",
        "crates/psychevo-gateway-protocol",
    ),
    ("psychevo-gateway", "crates/psychevo-gateway"),
    ("psychevo-acp", "crates/psychevo-acp"),
    ("psychevo-cli", "crates/psychevo-cli"),
];

pub(crate) fn check_sdk_architecture(root: &Path) -> Result<()> {
    let workspace = manifest(&root.join("Cargo.toml"))?;
    let members = string_array(value_at(&workspace, &["workspace", "members"])?)?;
    let expected_members = PRODUCT_CRATES
        .iter()
        .map(|(_, path)| (*path).to_string())
        .chain(std::iter::once("xtask".to_string()))
        .collect::<BTreeSet<_>>();
    if members != expected_members {
        bail!(
            "workspace members do not match the seven product crates plus xtask: expected {expected_members:?}, found {members:?}"
        );
    }

    let manifests = PRODUCT_CRATES
        .iter()
        .map(|(name, path)| Ok((*name, manifest(&root.join(path).join("Cargo.toml"))?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let expected_graph = BTreeMap::from([
        ("psychevo-ai", BTreeSet::new()),
        ("psychevo-agent-core", BTreeSet::from(["psychevo-ai"])),
        (
            "psychevo",
            BTreeSet::from(["psychevo-agent-core", "psychevo-ai"]),
        ),
        ("psychevo-gateway-protocol", BTreeSet::new()),
        (
            "psychevo-gateway",
            BTreeSet::from(["psychevo", "psychevo-gateway-protocol"]),
        ),
        ("psychevo-acp", BTreeSet::from(["psychevo"])),
        (
            "psychevo-cli",
            BTreeSet::from(["psychevo", "psychevo-acp", "psychevo-gateway"]),
        ),
    ]);
    let expected_version = value_at(&manifests["psychevo"], &["package", "version"])?
        .as_str()
        .context("psychevo package.version must be a string")?;

    for (name, manifest) in &manifests {
        let package_name = value_at(manifest, &["package", "name"])?
            .as_str()
            .context("package.name must be a string")?;
        if package_name != *name {
            bail!("manifest for {name} declares package name {package_name}");
        }
        if value_at(manifest, &["package", "version"])?.as_str() != Some(expected_version) {
            bail!("{name} must use the shared SDK product version {expected_version}");
        }
        let product_dependencies = dependency_table(manifest)
            .keys()
            .map(String::as_str)
            .filter(|dependency| manifests.contains_key(dependency))
            .collect::<BTreeSet<_>>();
        if product_dependencies != expected_graph[name] {
            bail!(
                "{name} product dependencies must be {:?}, found {product_dependencies:?}",
                expected_graph[name]
            );
        }
        for forbidden in ["opentelemetry", "tracing-opentelemetry"] {
            if dependency_table(manifest).contains_key(forbidden) {
                bail!("{name} must not depend on outbound telemetry crate {forbidden}");
            }
        }
    }

    for published in ["psychevo-ai", "psychevo-agent-core", "psychevo"] {
        let manifest = &manifests[published];
        if value_at_optional(manifest, &["package", "publish"]).is_some() {
            bail!("{published} must remain publishable");
        }
        for dependency in dependency_table(manifest)
            .iter()
            .filter(|(name, _)| manifests.contains_key(name.as_str()))
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
        || !framework_features.contains_key("internal")
    {
        bail!("psychevo must have an empty default feature set and an internal bridge feature");
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
        if !features.contains("internal") {
            bail!("{private} must opt into psychevo's internal bridge feature");
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

    #[test]
    fn current_workspace_has_the_sdk_architecture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        check_sdk_architecture(root).expect("SDK architecture");
    }
}
