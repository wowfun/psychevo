use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use toml::Value;

pub(crate) fn check_desktop_manifest_parity(root: &Path) -> Result<()> {
    let root_manifest = read_manifest(&root.join("Cargo.toml"))?;
    let desktop_manifest = read_manifest(&root.join("apps/desktop/src-tauri/Cargo.toml"))?;
    check_manifests(&root_manifest, &desktop_manifest)
}

fn read_manifest(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read Cargo manifest {}", path.display()))?;
    text.parse::<Value>()
        .with_context(|| format!("parse Cargo manifest {}", path.display()))
}

fn check_manifests(root: &Value, desktop: &Value) -> Result<()> {
    for field in ["edition", "rust-version", "license"] {
        let expected = value_at(root, &["workspace", "package", field])?;
        let actual = value_at(desktop, &["package", field])?;
        if expected != actual {
            bail!(
                "Desktop package {field} mismatch: root workspace has {expected}, Desktop has {actual}"
            );
        }
    }

    let root_resolver = value_at(root, &["workspace", "resolver"])?;
    let desktop_resolver = value_at(desktop, &["workspace", "resolver"])?;
    if root_resolver != desktop_resolver {
        bail!(
            "Desktop workspace resolver mismatch: root has {root_resolver}, Desktop has {desktop_resolver}"
        );
    }

    let root_dependencies = table_at(root, &["workspace", "dependencies"])?;
    for (name, dependency) in desktop_dependencies(desktop) {
        let Some(root_dependency) = root_dependencies.get(&name) else {
            continue;
        };
        let expected = dependency_contract(root_dependency)?;
        let actual = dependency_contract(dependency)?;
        if expected != actual {
            bail!(
                "Desktop dependency `{name}` mismatch: root has version `{}` default-features={}, Desktop has version `{}` default-features={}",
                expected.version,
                expected.default_features,
                actual.version,
                actual.default_features
            );
        }
    }
    Ok(())
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| format!("missing Cargo manifest field {}", path.join(".")))?;
    }
    Ok(current)
}

fn table_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a toml::map::Map<String, Value>> {
    value_at(value, path)?
        .as_table()
        .with_context(|| format!("Cargo manifest field {} is not a table", path.join(".")))
}

fn desktop_dependencies(desktop: &Value) -> BTreeMap<String, &Value> {
    let mut dependencies = BTreeMap::new();
    for section in ["dependencies", "build-dependencies"] {
        if let Some(table) = desktop.get(section).and_then(Value::as_table) {
            dependencies.extend(table.iter().map(|(name, value)| (name.clone(), value)));
        }
    }
    if let Some(targets) = desktop.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            for section in ["dependencies", "build-dependencies"] {
                if let Some(table) = target.get(section).and_then(Value::as_table) {
                    dependencies.extend(table.iter().map(|(name, value)| (name.clone(), value)));
                }
            }
        }
    }
    dependencies
}

#[derive(Debug, Eq, PartialEq)]
struct DependencyContract<'a> {
    version: &'a str,
    default_features: bool,
}

fn dependency_contract(value: &Value) -> Result<DependencyContract<'_>> {
    match value {
        Value::String(version) => Ok(DependencyContract {
            version,
            default_features: true,
        }),
        Value::Table(table) => Ok(DependencyContract {
            version: table
                .get("version")
                .and_then(Value::as_str)
                .context("workspace-shared dependency is missing a version")?,
            default_features: table
                .get("default-features")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }),
        _ => bail!("dependency declaration must be a version string or table"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests(desktop_reqwest: &str) -> (Value, Value) {
        let root = r#"
[workspace]
resolver = "3"
[workspace.package]
edition = "2024"
rust-version = "1.97.0"
license = "MIT"
[workspace.dependencies]
serde = "1.0"
reqwest = { version = "0.13.3", default-features = false, features = ["json"] }
"#;
        let desktop = format!(
            r#"
[package]
edition = "2024"
rust-version = "1.97.0"
license = "MIT"
[workspace]
resolver = "3"
[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
reqwest = {desktop_reqwest}
"#
        );
        (
            root.parse().expect("root manifest"),
            desktop.parse().expect("desktop manifest"),
        )
    }

    #[test]
    fn accepts_matching_metadata_and_dependency_contracts() {
        let (root, desktop) = manifests(r#"{ version = "0.13.3", default-features = false }"#);
        check_manifests(&root, &desktop).expect("matching manifests");
    }

    #[test]
    fn rejects_version_mismatch() {
        let (root, desktop) = manifests(r#"{ version = "0.12", default-features = false }"#);
        let error = check_manifests(&root, &desktop).expect_err("version mismatch");
        assert!(error.to_string().contains("reqwest"));
    }

    #[test]
    fn rejects_default_feature_mismatch() {
        let (root, desktop) = manifests(r#"{ version = "0.13.3" }"#);
        let error = check_manifests(&root, &desktop).expect_err("feature mismatch");
        assert!(error.to_string().contains("default-features"));
    }
}
