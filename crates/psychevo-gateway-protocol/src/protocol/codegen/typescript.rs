fn write_checked(path: &Path, content: &str, check: bool) -> Result<()> {
    if check {
        let existing = fs::read_to_string(path).with_context(|| {
            format!(
                "generated file is missing or unreadable: {}",
                path.display()
            )
        })?;
        if existing != content {
            bail!("generated file is out of date: {}", path.display());
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn ts_decl<T>() -> Result<String>
where
    T: TS,
{
    let decl = T::decl();
    Ok(export_ts_decl(decl))
}

fn export_ts_decl(decl: String) -> String {
    if decl.starts_with("type ") || decl.starts_with("interface ") {
        format!("export {decl}")
    } else {
        decl
    }
}

fn typescript_decl_with_schema_optionality(decl: &str, schema: &Value) -> String {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return decl.to_string();
    };
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let optional = properties
        .keys()
        .filter(|name| !required.contains(name.as_str()))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if optional.is_empty() {
        return decl.to_string();
    }

    let mut rendered = String::with_capacity(decl.len() + optional.len());
    let mut depth = 0_u32;
    let mut member_start = false;
    let mut member_name = String::new();
    for character in decl.chars() {
        match character {
            '{' => {
                depth += 1;
                member_start = depth == 1;
                member_name.clear();
                rendered.push(character);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                member_start = false;
                member_name.clear();
                rendered.push(character);
            }
            ',' if depth == 1 => {
                member_start = true;
                member_name.clear();
                rendered.push(character);
            }
            ':' if depth == 1 && member_start && !member_name.is_empty() => {
                if optional.contains(member_name.as_str()) && !rendered.ends_with('?') {
                    rendered.push('?');
                }
                rendered.push(character);
                member_start = false;
                member_name.clear();
            }
            '?' if depth == 1 && member_start => {
                rendered.push(character);
                member_start = false;
                member_name.clear();
            }
            character if depth == 1 && member_start => {
                if character.is_ascii_alphanumeric() || character == '_' || character == '$' {
                    member_name.push(character);
                } else if !character.is_whitespace() {
                    member_start = false;
                    member_name.clear();
                }
                rendered.push(character);
            }
            _ => rendered.push(character),
        }
    }
    rendered
}

fn schema<T>() -> Result<Value>
where
    T: JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T)).map_err(Into::into)
}

macro_rules! exported_type {
    ($ty:ty) => {
        ExportedType {
            name: stringify!($ty),
            ts_decl: ts_decl::<$ty>,
            schema: schema::<$ty>,
        }
    };
}
