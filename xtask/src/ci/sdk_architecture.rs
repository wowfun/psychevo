use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml::Value;

const HIDDEN_FACADE_IDENTIFIERS: &[&str] = &["__product", "__ai", "__agent_core"];
const FRAMEWORK_IMPLEMENTATION_MODULES: &[&str] = &["run", "state", "store"];

pub(crate) fn check_sdk_architecture(root: &Path) -> Result<()> {
    let workspace = manifest(&root.join("Cargo.toml"))?;
    let member_paths = string_array(value_at(&workspace, &["workspace", "members"])?)?;
    let mut manifests = BTreeMap::new();
    let mut member_roots = BTreeMap::new();
    for member_path in member_paths {
        let member_root = root.join(&member_path);
        let member = manifest(&member_root.join("Cargo.toml"))?;
        let name = value_at(&member, &["package", "name"])?
            .as_str()
            .context("workspace package.name must be a string")?
            .to_string();
        if manifests.insert(name.clone(), member).is_some() {
            bail!("workspace contains duplicate package name {name}");
        }
        member_roots.insert(name, member_root);
    }

    let workspace_manifests = manifests
        .iter()
        .filter(|(name, _)| {
            is_production_crate(
                member_roots
                    .get(*name)
                    .expect("workspace member root exists for every manifest"),
                root,
            )
        })
        .map(|(name, manifest)| (name.clone(), manifest))
        .collect::<BTreeMap<_, _>>();
    let workspace_names = workspace_manifests.keys().cloned().collect::<BTreeSet<_>>();
    let graph = workspace_manifests
        .iter()
        .map(|(name, manifest)| {
            (
                name.clone(),
                workspace_dependencies(manifest, &workspace_names),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let layers = workspace_manifests
        .iter()
        .map(|(name, package)| Ok((name.clone(), architecture_layer(name, package)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    validate_dependency_topology(&graph, &layers)?;
    validate_framework_consumer_dependencies(&graph)?;

    let framework = manifests
        .get("psychevo")
        .context("workspace is missing the published Framework crate psychevo")?;
    let expected_version = value_at(framework, &["package", "version"])?
        .as_str()
        .context("psychevo package.version must be a string")?;
    for (name, package) in &workspace_manifests {
        if value_at(package, &["package", "version"])?.as_str() != Some(expected_version) {
            bail!("{name} must use the shared SDK product version {expected_version}");
        }
    }
    for (name, package) in &workspace_manifests {
        for forbidden in ["opentelemetry", "tracing-opentelemetry"] {
            if dependency_table(package).contains_key(forbidden) {
                bail!("{name} must not depend on outbound telemetry crate {forbidden}");
            }
        }
    }

    for (published, package) in workspace_manifests
        .iter()
        .filter(|(_, package)| package_is_publishable(package))
    {
        for dependency in dependency_table(package)
            .iter()
            .filter(|(name, value)| {
                let package = dependency_package_name(name, value);
                workspace_names.contains(package)
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
                    "{published} workspace dependencies need both path and released version declarations"
                );
            }
        }
    }

    validate_framework_features(framework)?;
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
    let production_roots = workspace_manifests
        .keys()
        .map(|name| (name.clone(), member_roots[name].clone()))
        .collect::<BTreeMap<_, _>>();
    validate_source_architecture(root, &production_roots, &workspace_manifests, &graph)?;
    Ok(())
}

fn is_production_crate(member_root: &Path, workspace_root: &Path) -> bool {
    member_root
        .strip_prefix(workspace_root)
        .is_ok_and(|relative| relative.starts_with("crates"))
}

fn architecture_layer(name: &str, manifest: &Value) -> Result<usize> {
    let layer = value_at(
        manifest,
        &["package", "metadata", "psychevo", "architecture-layer"],
    )?
    .as_integer()
    .with_context(|| {
        format!("{name} package.metadata.psychevo.architecture-layer must be an integer")
    })?;
    usize::try_from(layer)
        .with_context(|| format!("{name} architecture layer must be non-negative"))
}

fn package_is_publishable(manifest: &Value) -> bool {
    !matches!(
        value_at_optional(manifest, &["package", "publish"]),
        Some(Value::Boolean(false))
    )
}

fn framework_consumers(graph: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    graph
        .keys()
        .filter(|name| name.as_str() != "psychevo")
        .filter(|name| reachable_dependencies(graph, name).contains("psychevo"))
        .cloned()
        .collect()
}

fn validate_framework_consumer_dependencies(
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    let framework_internals = reachable_dependencies(graph, "psychevo");
    for consumer in framework_consumers(graph) {
        for dependency in &graph[&consumer] {
            if framework_internals.contains(dependency) {
                bail!(
                    "{consumer} must use the Framework interface instead of depending directly on {dependency}"
                );
            }
        }
    }
    Ok(())
}

fn validate_framework_features(framework: &Value) -> Result<()> {
    let Some(features) = value_at_optional(framework, &["features"]) else {
        return Ok(());
    };
    let features = features
        .as_table()
        .context("psychevo features must be a table")?;
    if !features
        .get("default")
        .is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
    {
        bail!("psychevo must have an empty default feature set");
    }
    for (name, value) in features {
        if name == "default" {
            continue;
        }
        let members = value
            .as_array()
            .with_context(|| format!("psychevo feature `{name}` must be an array"))?;
        if members.is_empty() {
            bail!("psychevo has empty taxonomy feature `{name}`");
        }
    }
    Ok(())
}

fn validate_source_architecture(
    root: &Path,
    production_roots: &BTreeMap<String, PathBuf>,
    manifests: &BTreeMap<String, &Value>,
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    for (name, crate_root) in production_roots {
        for path in rust_files(crate_root)? {
            let source = read_source(&path)?;
            validate_no_hidden_facade(&source).with_context(|| source_label(root, &path))?;
        }
        for path in crate_entrypoints(crate_root, manifests[name])? {
            let source = read_source(&path)?;
            validate_crate_root_surface(name, &source).with_context(|| {
                format!(
                    "{} violates its crate-root boundary",
                    source_label(root, &path)
                )
            })?;
        }
    }

    for adapter in framework_consumers(graph) {
        for path in rust_files(&production_roots[&adapter])? {
            let source = read_source(&path)?;
            validate_adapter_source(&source).with_context(|| source_label(root, &path))?;
        }
    }

    let mut source_roots = production_roots.values().cloned().collect::<Vec<_>>();
    source_roots.push(root.join("apps/desktop/src-tauri/src"));
    for source_root in source_roots {
        for path in production_rust_files(&source_root)? {
            let source = read_source(&path)?;
            if let Some(statement) = handwritten_include(&source) {
                bail!(
                    "{} incorporates handwritten production source with `{statement}`; declare a Rust module instead",
                    source_label(root, &path)
                );
            }
            validate_no_glob_reexport(&source).with_context(|| source_label(root, &path))?;
            validate_no_shared_import_environment(&source)
                .with_context(|| source_label(root, &path))?;
        }
    }
    validate_cli_test_imports(root)?;
    Ok(())
}

fn validate_cli_test_imports(root: &Path) -> Result<()> {
    for source_root in [
        root.join("crates/psychevo-cli/src/tui/tests"),
        root.join("crates/psychevo-cli/tests"),
    ] {
        for path in rust_files(&source_root)? {
            let source = read_source(&path)?;
            validate_explicit_test_import_environment(&source)
                .with_context(|| source_label(root, &path))?;
        }
    }
    Ok(())
}

fn validate_crate_root_surface(crate_name: &str, source: &str) -> Result<()> {
    validate_no_hidden_facade(source)?;
    if glob_reexport_statement(source).is_some() {
        bail!("{crate_name} crate root must not expose a glob re-export");
    }
    Ok(())
}

fn validate_no_glob_reexport(source: &str) -> Result<()> {
    if let Some(statement) = glob_reexport_statement(source) {
        bail!(
            "production module re-exports a wildcard namespace with `{statement}`; export owned names explicitly"
        );
    }
    Ok(())
}

fn glob_reexport_statement(source: &str) -> Option<&str> {
    let tokens = rust_tokens(&rust_code_mask(source));
    let test_only_ranges = cfg_test_item_ranges(&tokens);
    for (index, token) in tokens.iter().enumerate() {
        if token.text != "pub"
            || test_only_ranges
                .iter()
                .any(|(start, end)| (*start..*end).contains(&token.start))
        {
            continue;
        }
        let mut cursor = index + 1;
        if tokens.get(cursor).is_some_and(|next| next.text == "(") {
            let Some(visibility_end) = matching_token(&tokens, cursor, "(", ")") else {
                continue;
            };
            cursor = visibility_end + 1;
        }
        if tokens.get(cursor).is_none_or(|next| next.text != "use") {
            continue;
        }
        let Some(semicolon) = tokens[cursor + 1..]
            .iter()
            .position(|next| next.text == ";")
            .map(|offset| cursor + 1 + offset)
        else {
            continue;
        };
        if tokens[cursor + 1..semicolon]
            .iter()
            .any(|next| next.text == "*")
        {
            return Some(source[token.start..tokens[semicolon].start + 1].trim());
        }
    }
    None
}

fn validate_no_hidden_facade(source: &str) -> Result<()> {
    let tokens = rust_tokens(&rust_code_mask(source));
    for forbidden in HIDDEN_FACADE_IDENTIFIERS {
        if tokens.iter().any(|token| token.text == *forbidden) {
            bail!("retired hidden facade identifier `{forbidden}` remains in Rust source");
        }
    }
    Ok(())
}

fn validate_adapter_source(source: &str) -> Result<()> {
    let tokens = rust_tokens(&rust_code_mask(source));
    for (index, token) in tokens.iter().enumerate() {
        if token.text != "psychevo" || tokens.get(index + 1).is_none_or(|next| next.text != "::") {
            continue;
        }
        let Some(next) = tokens.get(index + 2) else {
            continue;
        };
        if FRAMEWORK_IMPLEMENTATION_MODULES.contains(&next.text.as_str()) {
            bail!(
                "adapter imports Framework implementation module `psychevo::{}`",
                next.text
            );
        }
        if next.text != "{" {
            continue;
        }
        let mut depth = 1usize;
        for candidate in &tokens[index + 3..] {
            match candidate.text.as_str() {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                name if depth == 1 && FRAMEWORK_IMPLEMENTATION_MODULES.contains(&name) => {
                    bail!("adapter imports Framework implementation module `psychevo::{name}`");
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handwritten_include(source: &str) -> Option<String> {
    let masked = rust_code_mask(source);
    let tokens = rust_tokens(&masked);
    let test_only_ranges = cfg_test_item_ranges(&tokens);
    for (index, token) in tokens.iter().enumerate() {
        if token.text != "include"
            || tokens.get(index + 1).is_none_or(|next| next.text != "!")
            || tokens.get(index + 2).is_none_or(|next| next.text != "(")
            || test_only_ranges
                .iter()
                .any(|(start, end)| (*start..*end).contains(&token.start))
        {
            continue;
        }
        let end = tokens[index + 3..]
            .iter()
            .find(|next| next.text == ";")
            .map_or(source.len(), |next| next.start + 1);
        let statement = source[token.start..end].trim();
        if !uses_cargo_out_dir(statement) {
            return Some(statement.to_string());
        }
    }
    None
}

fn validate_no_shared_import_environment(source: &str) -> Result<()> {
    let tokens = rust_tokens(&rust_code_mask(source));
    let test_only_ranges = cfg_test_item_ranges(&tokens);
    validate_import_environment(&tokens, &test_only_ranges, false)
}

fn validate_explicit_test_import_environment(source: &str) -> Result<()> {
    let tokens = rust_tokens(&rust_code_mask(source));
    validate_import_environment(&tokens, &[], true)
}

fn validate_import_environment(
    tokens: &[RustToken],
    ignored_ranges: &[(usize, usize)],
    reject_all_globs: bool,
) -> Result<()> {
    let is_ignored = |token: &RustToken| {
        ignored_ranges
            .iter()
            .any(|(start, end)| (*start..*end).contains(&token.start))
    };

    for (index, token) in tokens.iter().enumerate() {
        if is_ignored(token) {
            continue;
        }

        if token.text == "use" {
            let statement = tokens[index + 1..]
                .iter()
                .take_while(|candidate| candidate.text != ";")
                .collect::<Vec<_>>();
            let inherits_parent = statement.windows(3).any(|window| {
                window[0].text == "super" && window[1].text == "::" && window[2].text == "*"
            });
            let has_glob = statement.iter().any(|candidate| candidate.text == "*");
            if inherits_parent {
                bail!(
                    "module inherits its parent import environment with `use super::*`; import the exercised seams explicitly"
                );
            }
            if reject_all_globs && has_glob {
                bail!("test module uses a wildcard import; import the exercised seams explicitly");
            }
        }

        if token.text != "#" {
            continue;
        }
        let bracket = if tokens.get(index + 1).is_some_and(|next| next.text == "!") {
            index + 2
        } else {
            index + 1
        };
        if tokens
            .get(bracket)
            .is_none_or(|candidate| candidate.text != "[")
        {
            continue;
        }
        let Some(attribute_end) = matching_token(tokens, bracket, "[", "]") else {
            continue;
        };
        let attribute = &tokens[bracket + 1..attribute_end];
        if let Some(lint) = suppressed_shared_namespace_lint(attribute, reject_all_globs) {
            bail!(
                "module suppresses `{}` for a shared import environment; remove the dead seam or import only what the module owns",
                lint.text
            );
        }
    }
    Ok(())
}

fn suppressed_shared_namespace_lint(
    attribute: &[RustToken],
    test_source: bool,
) -> Option<&RustToken> {
    let allow_start = match attribute.first()?.text.as_str() {
        "allow" if attribute.get(1).is_some_and(|token| token.text == "(") => 0,
        "cfg_attr" if attribute.get(1).is_some_and(|token| token.text == "(") => {
            let predicate_end = top_level_comma(attribute, 2)?;
            if !test_source
                && attribute[2..predicate_end].starts_with_text(&["test"])
                && predicate_end == 3
            {
                return None;
            }
            attribute[predicate_end + 1..]
                .iter()
                .position(|token| token.text == "allow")?
                + predicate_end
                + 1
        }
        _ => return None,
    };
    if attribute
        .get(allow_start + 1)
        .is_none_or(|token| token.text != "(")
    {
        return None;
    }
    let allow_end = matching_token(attribute, allow_start + 1, "(", ")")?;
    attribute[allow_start + 2..allow_end]
        .iter()
        .find(|token| matches!(token.text.as_str(), "unused_imports" | "dead_code"))
}

fn top_level_comma(tokens: &[RustToken], start: usize) -> Option<usize> {
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" => parentheses += 1,
            ")" if parentheses == 0 => return None,
            ")" => parentheses -= 1,
            "[" => brackets += 1,
            "]" => brackets = brackets.saturating_sub(1),
            "," if parentheses == 0 && brackets == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn uses_cargo_out_dir(statement: &str) -> bool {
    let masked = rust_code_mask(statement);
    let tokens = rust_tokens(&masked);
    for (index, token) in tokens.iter().enumerate() {
        if token.text != "env"
            || tokens.get(index + 1).is_none_or(|next| next.text != "!")
            || tokens.get(index + 2).is_none_or(|next| next.text != "(")
        {
            continue;
        }
        let Some(close) = matching_token(&tokens, index + 2, "(", ")") else {
            continue;
        };
        let argument = statement[tokens[index + 2].start + 1..tokens[close].start].trim();
        if argument == "\"OUT_DIR\"" {
            return true;
        }
    }
    false
}

fn cfg_test_item_ranges(tokens: &[RustToken]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index + 5 < tokens.len() {
        if !tokens[index..].starts_with_text(&["#", "[", "cfg", "("]) {
            index += 1;
            continue;
        }
        let Some(cfg_end) = matching_token(tokens, index + 3, "(", ")") else {
            index += 1;
            continue;
        };
        if tokens
            .get(cfg_end + 1)
            .is_none_or(|token| token.text != "]")
            || !cfg_predicate_requires_test(&tokens[index + 4..cfg_end])
        {
            index += 1;
            continue;
        }

        let mut cursor = cfg_end + 2;
        while tokens.get(cursor).is_some_and(|token| token.text == "#")
            && tokens
                .get(cursor + 1)
                .is_some_and(|token| token.text == "[")
        {
            let Some(end) = matching_token(tokens, cursor + 1, "[", "]") else {
                break;
            };
            cursor = end + 1;
        }
        if tokens.get(cursor).is_some_and(|token| token.text == "pub") {
            cursor += 1;
            if tokens.get(cursor).is_some_and(|token| token.text == "(") {
                let Some(end) = matching_token(tokens, cursor, "(", ")") else {
                    index += 1;
                    continue;
                };
                cursor = end + 1;
            }
        }
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        let mut item_end = None;
        for candidate in cursor..tokens.len() {
            match tokens[candidate].text.as_str() {
                "(" => parentheses += 1,
                ")" => parentheses = parentheses.saturating_sub(1),
                "[" => brackets += 1,
                "]" => brackets = brackets.saturating_sub(1),
                "{" if parentheses == 0 && brackets == 0 => {
                    item_end = matching_token(tokens, candidate, "{", "}");
                    break;
                }
                ";" if parentheses == 0 && brackets == 0 => {
                    item_end = Some(candidate);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = item_end else {
            index += 1;
            continue;
        };
        ranges.push((tokens[index].start, tokens[end].start + 1));
        index = end + 1;
    }
    ranges
}

fn cfg_predicate_requires_test(tokens: &[RustToken]) -> bool {
    let mut cursor = 0;
    let Some((can_be_true, _can_be_false)) = cfg_predicate_domain(tokens, &mut cursor) else {
        return false;
    };
    cursor == tokens.len() && !can_be_true
}

/// Return whether a cfg predicate can be true or false when `cfg(test)` is
/// fixed to false. Unknown platform, target, and feature atoms may take either
/// value; this makes the test-only classification conservative.
fn cfg_predicate_domain(tokens: &[RustToken], cursor: &mut usize) -> Option<(bool, bool)> {
    let name = tokens.get(*cursor)?.text.as_str();
    *cursor += 1;
    if tokens.get(*cursor).is_none_or(|token| token.text != "(") {
        return Some(if name == "test" {
            (false, true)
        } else {
            (true, true)
        });
    }

    *cursor += 1;
    let mut arguments = Vec::new();
    while tokens.get(*cursor).is_some_and(|token| token.text != ")") {
        arguments.push(cfg_predicate_domain(tokens, cursor)?);
        if tokens.get(*cursor).is_some_and(|token| token.text == ",") {
            *cursor += 1;
        } else if tokens.get(*cursor).is_none_or(|token| token.text != ")") {
            return None;
        }
    }
    if tokens.get(*cursor).is_none_or(|token| token.text != ")") {
        return None;
    }
    *cursor += 1;

    match name {
        "all" => Some((
            arguments.iter().all(|(can_be_true, _)| *can_be_true),
            arguments.iter().any(|(_, can_be_false)| *can_be_false),
        )),
        "any" => Some((
            arguments.iter().any(|(can_be_true, _)| *can_be_true),
            arguments.iter().all(|(_, can_be_false)| *can_be_false),
        )),
        "not" if arguments.len() == 1 => Some((arguments[0].1, arguments[0].0)),
        _ => Some((true, true)),
    }
}

fn matching_token(
    tokens: &[RustToken],
    open: usize,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.text == opening {
            depth += 1;
        } else if token.text == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

trait RustTokenSliceExt {
    fn starts_with_text(&self, expected: &[&str]) -> bool;
}

impl RustTokenSliceExt for [RustToken] {
    fn starts_with_text(&self, expected: &[&str]) -> bool {
        self.len() >= expected.len()
            && self
                .iter()
                .zip(expected)
                .all(|(token, expected)| token.text == *expected)
    }
}

#[derive(Debug)]
struct RustToken {
    text: String,
    start: usize,
}

fn rust_tokens(source: &str) -> Vec<RustToken> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(RustToken {
                text: source[start..index].to_string(),
                start,
            });
            continue;
        }
        if byte == b':' && bytes.get(index + 1) == Some(&b':') {
            tokens.push(RustToken {
                text: "::".to_string(),
                start: index,
            });
            index += 2;
            continue;
        }
        if matches!(
            byte,
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b'#' | b'*' | b'!' | b';' | b','
        ) {
            tokens.push(RustToken {
                text: char::from(byte).to_string(),
                start: index,
            });
        }
        index += 1;
    }
    tokens
}

fn rust_code_mask(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            mask_range(&mut masked, start, index);
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            mask_range(&mut masked, start, index);
            continue;
        }
        if let Some((quote, hashes)) = raw_string_start(bytes, index) {
            let start = index;
            index = quote + 1;
            while index < bytes.len() {
                if bytes[index] == b'"'
                    && (0..hashes).all(|offset| bytes.get(index + 1 + offset) == Some(&b'#'))
                {
                    index += 1 + hashes;
                    break;
                }
                index += 1;
            }
            mask_range(&mut masked, start, index);
            continue;
        }
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            mask_range(&mut masked, start, index);
            continue;
        }
        index += 1;
    }
    String::from_utf8(masked).expect("mask preserves UTF-8 byte layout")
}

fn raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let mut hashes = 0usize;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some((cursor, hashes))
}

fn mask_range(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

fn production_rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let all_files = rust_files(root)?;
    let available = all_files.iter().cloned().collect::<BTreeSet<_>>();
    let mut pending = all_files
        .iter()
        .filter(|path| is_crate_entrypoint(path))
        .cloned()
        .collect::<Vec<_>>();
    let mut production = BTreeSet::new();

    while let Some(path) = pending.pop() {
        if !production.insert(path.clone()) {
            continue;
        }
        let source = read_source(&path)?;
        pending.extend(external_production_modules(&path, &source, &available));
    }

    Ok(production.into_iter().collect())
}

fn crate_entrypoints(crate_root: &Path, manifest: &Value) -> Result<Vec<PathBuf>> {
    let mut entrypoints = BTreeSet::new();
    let source_root = crate_root.join("src");
    if source_root.exists() {
        entrypoints.extend(
            rust_files(&source_root)?
                .into_iter()
                .filter(|path| is_crate_entrypoint(path)),
        );
    }

    if let Some(path) = value_at_optional(manifest, &["lib", "path"]).and_then(Value::as_str) {
        entrypoints.insert(crate_root.join(path));
    }
    if let Some(binaries) = manifest.get("bin").and_then(Value::as_array) {
        for path in binaries.iter().filter_map(|binary| {
            binary
                .as_table()
                .and_then(|table| table.get("path"))
                .and_then(Value::as_str)
        }) {
            entrypoints.insert(crate_root.join(path));
        }
    }

    Ok(entrypoints
        .into_iter()
        .filter(|path| path.is_file())
        .collect())
}

fn is_crate_entrypoint(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let Some(src) = components.iter().rposition(|component| *component == "src") else {
        return false;
    };
    let relative = &components[src + 1..];
    matches!(relative, ["lib.rs"] | ["main.rs"])
        || matches!(relative, ["bin", file] if file.ends_with(".rs"))
        || matches!(relative, ["bin", _, "main.rs"])
}

fn external_production_modules(
    source_path: &Path,
    source: &str,
    available: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let masked = rust_code_mask(source);
    let tokens = rust_tokens(&masked);
    let test_only_ranges = cfg_test_item_ranges(&tokens);
    let mut modules = BTreeSet::new();

    for (index, token) in tokens.iter().enumerate() {
        if token.text != "mod"
            || test_only_ranges
                .iter()
                .any(|(start, end)| (*start..*end).contains(&token.start))
        {
            continue;
        }
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        if tokens.get(index + 2).is_none_or(|next| next.text != ";") {
            continue;
        }

        if let Some(path) = module_path_override(source_path, source, &tokens, index) {
            if available.contains(&path) {
                modules.insert(path);
            }
            continue;
        }

        let mut base = conventional_module_base(source_path);
        for inline_module in containing_inline_modules(&tokens, index, &test_only_ranges) {
            base.push(inline_module);
        }
        for candidate in [
            base.join(format!("{}.rs", name.text)),
            base.join(&name.text).join("mod.rs"),
        ] {
            if available.contains(&candidate) {
                modules.insert(candidate);
                break;
            }
        }
    }
    modules.into_iter().collect()
}

fn containing_inline_modules(
    tokens: &[RustToken],
    item_index: usize,
    test_only_ranges: &[(usize, usize)],
) -> Vec<String> {
    let mut modules = Vec::new();
    for index in 0..item_index {
        if tokens[index].text != "mod"
            || test_only_ranges
                .iter()
                .any(|(start, end)| (*start..*end).contains(&tokens[index].start))
        {
            continue;
        }
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        if tokens.get(index + 2).is_none_or(|next| next.text != "{") {
            continue;
        }
        if matching_token(tokens, index + 2, "{", "}").is_some_and(|end| item_index < end) {
            modules.push(name.text.clone());
        }
    }
    modules
}

fn module_path_override(
    source_path: &Path,
    source: &str,
    tokens: &[RustToken],
    module_index: usize,
) -> Option<PathBuf> {
    let start = tokens[..module_index]
        .iter()
        .rposition(|token| matches!(token.text.as_str(), ";" | "{" | "}"))
        .map_or(0, |index| tokens[index].start + 1);
    let prefix = &source[start..tokens[module_index].start];
    let attribute = prefix.rsplit("#[").find(|attribute| {
        attribute
            .trim_start()
            .strip_prefix("path")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })?;
    let quote = attribute.find('"')?;
    let rest = &attribute[quote + 1..];
    let close = rest.find('"')?;
    source_path
        .parent()
        .map(|parent| parent.join(&rest[..close]))
}

fn conventional_module_base(source_path: &Path) -> PathBuf {
    let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if matches!(stem, "lib" | "main" | "mod") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    }
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("read source directory {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("read entry under {}", directory.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_rust_files(&path, files)?;
        } else if file_type.is_file() && path.extension().is_some_and(|value| value == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn read_source(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read Rust source {}", path.display()))
}

fn source_label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn validate_dependency_topology(
    graph: &BTreeMap<String, BTreeSet<String>>,
    layers: &BTreeMap<String, usize>,
) -> Result<()> {
    if graph.keys().ne(layers.keys()) {
        bail!("every production workspace crate must declare exactly one architecture layer");
    }
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

    for (owner, owner_layer) in layers {
        let reachable = reachable_dependencies(graph, owner);
        for dependency in reachable {
            let dependency_layer = layers
                .get(dependency.as_str())
                .expect("graph and architecture layers have identical members");
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

    fn layers(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|(name, layer)| ((*name).to_string(), *layer))
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
        let layers = layers(&[
            ("psychevo-ai", 0),
            ("psychevo-gateway-protocol", 0),
            ("psychevo-agent-core", 1),
            ("runtime-implementation", 2),
            ("psychevo", 2),
            ("psychevo-gateway", 3),
            ("psychevo-acp", 3),
            ("psychevo-cli", 4),
        ]);

        validate_dependency_topology(&graph, &layers).expect("valid inserted implementation layer");
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
        let layers = layers(&[
            ("psychevo-ai", 0),
            ("psychevo-gateway-protocol", 0),
            ("private-helper", 0),
            ("psychevo-agent-core", 1),
            ("psychevo", 2),
            ("psychevo-gateway", 3),
            ("psychevo-acp", 3),
            ("psychevo-cli", 4),
        ]);

        let error = validate_dependency_topology(&graph, &layers).expect_err("reverse dependency");
        assert!(error.to_string().contains("higher-layer psychevo-gateway"));
    }

    #[test]
    fn topology_rejects_cycles_through_internal_crates() {
        let graph = graph(&[
            ("psychevo-ai", &["private-helper"]),
            ("private-helper", &["psychevo-ai"]),
        ]);
        let layers = layers(&[("psychevo-ai", 0), ("private-helper", 0)]);

        let error = validate_dependency_topology(&graph, &layers).expect_err("cycle");
        assert!(error.to_string().contains("dependency cycle"));
    }

    #[test]
    fn framework_consumers_are_discovered_from_dependencies() {
        let consumer_graph = graph(&[
            ("psychevo-ai", &[]),
            ("psychevo-agent-core", &["psychevo-ai"]),
            ("psychevo", &["psychevo-agent-core"]),
            ("new-transport-adapter", &["psychevo"]),
            ("new-product-shell", &["new-transport-adapter"]),
        ]);

        assert_eq!(
            framework_consumers(&consumer_graph),
            ["new-product-shell", "new-transport-adapter"]
                .into_iter()
                .map(ToString::to_string)
                .collect()
        );

        let invalid = graph(&[
            ("psychevo-ai", &[]),
            ("psychevo-agent-core", &["psychevo-ai"]),
            ("psychevo", &["psychevo-agent-core"]),
            ("new-transport-adapter", &["psychevo", "psychevo-ai"]),
        ]);
        let error = validate_framework_consumer_dependencies(&invalid)
            .expect_err("adapter bypasses Framework");
        assert!(error.to_string().contains("new-transport-adapter"));
        assert!(error.to_string().contains("psychevo-ai"));
    }

    #[test]
    fn architecture_layer_comes_from_package_metadata() {
        let manifest = r#"
            [package]
            name = "new-adapter"
            [package.metadata.psychevo]
            architecture-layer = 3
        "#
        .parse::<Value>()
        .expect("manifest fixture");
        assert_eq!(architecture_layer("new-adapter", &manifest).unwrap(), 3);

        let missing = "[package]\nname = \"new-adapter\"\n"
            .parse::<Value>()
            .expect("manifest fixture");
        assert!(
            architecture_layer("new-adapter", &missing)
                .expect_err("missing layer")
                .to_string()
                .contains("package.metadata.psychevo.architecture-layer")
        );
    }

    #[test]
    fn framework_features_reject_empty_taxonomy_seams() {
        let absent = "[package]\nname = \"psychevo\"\n"
            .parse::<Value>()
            .expect("featureless fixture");
        validate_framework_features(&absent).expect("absent features are the empty feature set");

        let invalid = "[features]\ndefault = []\nproduct = []\n"
            .parse::<Value>()
            .expect("feature fixture");
        let error = validate_framework_features(&invalid).expect_err("empty feature");
        assert!(
            error
                .to_string()
                .contains("empty taxonomy feature `product`")
        );

        let valid = "[features]\ndefault = []\nnative = [\"dep:native\"]\n"
            .parse::<Value>()
            .expect("feature fixture");
        validate_framework_features(&valid).expect("cost-bearing feature");
    }

    #[test]
    fn every_crate_root_rejects_hidden_facades_and_root_globs() {
        let hidden = validate_crate_root_surface("new-adapter", "pub mod __product {}")
            .expect_err("hidden facade");
        assert!(hidden.to_string().contains("__product"));

        let glob = validate_crate_root_surface("new-adapter", "pub use crate::runtime::*;")
            .expect_err("root glob");
        assert!(glob.to_string().contains("new-adapter crate root"));

        validate_crate_root_surface(
            "new-adapter",
            r#"
            // pub mod __product {}
            const RETIRED_NAME: &str = "__agent_core";
            pub use application::{Application, Client};
            "#,
        )
        .expect("comments and strings are not Rust symbols");
    }

    #[test]
    fn production_modules_reject_glob_reexports_outside_test_only_code() {
        for source in [
            "pub use dependency::*;",
            "pub(crate) use crate::runtime::*;",
            "pub(in crate::runtime) use super::{owned::*};",
            r#"#[cfg(any(test, feature = "test-support"))] pub use fixtures::*;"#,
        ] {
            let error = validate_no_glob_reexport(source).expect_err("glob re-export");
            assert!(error.to_string().contains("wildcard namespace"));
        }

        validate_no_glob_reexport(
            r##"
            #[cfg(test)]
            mod tests {
                pub(crate) use super::*;
            }
            // pub use dependency::*;
            const EXAMPLE: &str = "pub use dependency::*;";
            pub use application::{Application, Client};
            "##,
        )
        .expect("test-only, comments, strings, and explicit exports are allowed");
    }

    #[test]
    fn adapter_boundary_rejects_framework_implementation_modules() {
        for source in [
            "use psychevo::state::StateRuntime;",
            "use psychevo::{Application, run::RunOptions};",
            "fn raw() { let _ = psychevo::store::open(); }",
        ] {
            let error = validate_adapter_source(source).expect_err("implementation import");
            assert!(
                error
                    .to_string()
                    .contains("Framework implementation module")
            );
        }
        validate_adapter_source("use psychevo::{Application, Client, Thread};")
            .expect("semantic Framework interface");
    }

    #[test]
    fn production_import_check_rejects_shared_parent_preludes_and_broad_suppression() {
        for (source, evidence) in [
            ("use super::*;", "use super::*"),
            ("pub(crate) use super::*;", "use super::*"),
            (
                "#[allow(unused_imports)] use crate::runtime::Owner;",
                "unused_imports",
            ),
            ("#![allow(dead_code)] fn retained() {}", "dead_code"),
            (
                "#![cfg_attr(not(feature = \"native\"), allow(dead_code))] fn retained() {}",
                "dead_code",
            ),
            (
                "#[cfg_attr(any(test, feature = \"test-support\"), allow(unused_imports))] use crate::runtime::Owner;",
                "unused_imports",
            ),
        ] {
            let error = validate_no_shared_import_environment(source)
                .expect_err("shared production import environment");
            assert!(error.to_string().contains(evidence));
        }
    }

    #[test]
    fn production_and_test_import_checks_apply_their_respective_cfg_domains() {
        let test_only_source = r##"
            use super::{OwnedState, OwnedTask};
            #[cfg(test)]
            #[allow(unused_imports)]
            mod tests {
                use super::*;
            }
            #[cfg_attr(test, allow(dead_code))]
            fn test_configuration_helper() {}
            #[cfg(all(test, unix))]
            mod unix_tests {
                use super::*;
            }
            // use super::*;
            const EXAMPLE: &str = "#[allow(dead_code)] use super::*;";
            "##;
        validate_no_shared_import_environment(test_only_source)
            .expect("production check ignores code unreachable outside cfg(test)");
        let test_error = validate_explicit_test_import_environment(test_only_source)
            .expect_err("the test-source pass checks cfg(test) code");
        assert!(
            test_error.to_string().contains("unused_imports")
                || test_error.to_string().contains("use super::*")
        );

        let mixed = validate_no_shared_import_environment(
            r#"
            #[cfg(any(test, feature = "test-support"))]
            mod support {
                use super::*;
            }
            "#,
        )
        .expect_err("a production feature keeps the shared import reachable");
        assert!(mixed.to_string().contains("use super::*"));
    }

    #[test]
    fn test_import_check_rejects_wildcards_and_broad_lint_suppression() {
        for (source, evidence) in [
            ("#[cfg(test)] mod tests { use super::*; }", "use super::*"),
            ("pub(crate) use fixtures::*;", "wildcard import"),
            (
                "#[allow(unused_imports)] use crate::runtime::Owner;",
                "unused_imports",
            ),
            (
                "#[cfg_attr(test, allow(unused_imports))] use crate::runtime::Owner;",
                "unused_imports",
            ),
            ("#![allow(dead_code)] fn retained() {}", "dead_code"),
        ] {
            let error = validate_explicit_test_import_environment(source)
                .expect_err("shared test import environment");
            assert!(error.to_string().contains(evidence));
        }

        validate_explicit_test_import_environment(
            "use super::{OwnedFixture, run_fixture};\nuse crate::runtime::Owner;",
        )
        .expect("explicit test imports");
    }

    #[test]
    fn production_include_check_allows_only_generated_out_dir_input() {
        let handwritten =
            handwritten_include("include!(\"protocol.rs\");").expect("handwritten include");
        assert!(handwritten.contains("protocol.rs"));

        assert!(
            handwritten_include(
                "include!(concat!(env!(\"OUT_DIR\"), \"/generated_protocol.rs\"));"
            )
            .is_none()
        );
        for handwritten in [
            "include!(\"OUT_DIR/manual.rs\");",
            "include!(concat!(\"OUT_DIR\", \"/manual.rs\"));",
            "include!(concat!(env!(\"OUT_DIR_SUFFIX\"), \"/manual.rs\"));",
        ] {
            assert!(
                handwritten_include(handwritten).is_some(),
                "a textual OUT_DIR mention is not generated output: {handwritten}"
            );
        }
        assert!(
            handwritten_include("const EXAMPLE: &str = \"include!(\\\"example.rs\\\");\";")
                .is_none()
        );
    }

    #[test]
    fn production_include_check_ignores_only_provably_test_only_source() {
        assert!(
            handwritten_include(
                r#"
                #[cfg(test)]
                #[allow(dead_code)]
                mod test_support {
                    include!("test_support.rs");
                }
                "#,
            )
            .is_none()
        );
        assert!(
            handwritten_include(
                r#"
                #[cfg(test)]
                include!("test_only_fixture.rs");
                "#,
            )
            .is_none(),
            "an include item compiled only by cfg(test) is not production"
        );
        assert!(
            handwritten_include(
                r#"
                #[cfg(test)]
                const TEST_VALUE: () = {
                    include!("test_only_expression.rs");
                };
                "#,
            )
            .is_none(),
            "an include nested in a cfg(test)-only item is not production"
        );

        let mixed = handwritten_include(
            r#"
            #[cfg(test)]
            mod tests { include!("tests.rs"); }
            include!("production.rs");
            "#,
        )
        .expect("production include remains visible");
        assert!(mixed.contains("production.rs"));

        assert!(
            handwritten_include(
                r#"
                #[cfg(any(test, feature = "test-support"))]
                mod support { include!("support.rs"); }
                "#,
            )
            .is_some(),
            "a module compiled by a production feature is not test-only"
        );
    }

    #[test]
    fn production_source_walk_uses_cfg_reachability_not_test_path_names() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "psychevo-sdk-architecture-{}-{nonce}",
            std::process::id()
        ));
        let source = root.join("crates/example/src");
        fs::create_dir_all(source.join("runtime/tests")).expect("test source tree");
        fs::create_dir_all(source.join("inline_runtime")).expect("inline module tree");
        fs::write(
            source.join("lib.rs"),
            concat!(
                "mod tests;\n",
                "mod runtime;\n",
                "mod inline_runtime { mod support; }\n",
                "#[cfg(test)] mod test_support;\n"
            ),
        )
        .expect("crate root");
        fs::write(source.join("tests.rs"), "include!(\"production.rs\");\n")
            .expect("production tests.rs");
        fs::write(source.join("runtime.rs"), "mod tests;\n").expect("runtime module");
        fs::write(
            source.join("runtime/tests/mod.rs"),
            "include!(\"production.rs\");\n",
        )
        .expect("production tests directory");
        fs::write(
            source.join("test_support.rs"),
            "include!(\"test_support.rs\");\n",
        )
        .expect("test-only module");
        fs::write(
            source.join("inline_runtime/support.rs"),
            "include!(\"production.rs\");\n",
        )
        .expect("inline production module");

        let production = production_rust_files(&root.join("crates"))
            .expect("walk production modules")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(production.contains(&source.join("tests.rs")));
        assert!(production.contains(&source.join("runtime/tests/mod.rs")));
        assert!(production.contains(&source.join("inline_runtime/support.rs")));
        assert!(!production.contains(&source.join("test_support.rs")));
        for path in [
            source.join("tests.rs"),
            source.join("runtime/tests/mod.rs"),
            source.join("inline_runtime/support.rs"),
        ] {
            let include = handwritten_include(&fs::read_to_string(path).expect("source"));
            assert!(include.is_some(), "production include must be rejected");
        }

        fs::remove_dir_all(&root).expect("remove architecture fixture");
    }
}
