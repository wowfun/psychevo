fn render_schema_export(name: &'static str, schema: &Value) -> Result<SchemaRender> {
    if !matches!(name, "ClientRequest" | "ServerNotification") {
        return Ok(SchemaRender {
            root_json: serde_json::to_string_pretty(schema)?,
            refs: Vec::new(),
        });
    }
    let variants = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .context("composite schema has no oneOf variants")?;
    let definitions = schema
        .get("definitions")
        .and_then(Value::as_object)
        .context("composite schema has no definitions")?;
    let mut refs = Vec::new();
    let mut root = schema.clone();
    let mut root_refs = Vec::new();
    for variant in variants {
        let method = composite_method(variant)?;
        let slug = schema_part_slug(method);
        let ref_id = format!("{name}/{slug}.json");
        root_refs.push(serde_json::json!({ "$ref": ref_id }));
        refs.push(SchemaRefEntry {
            export_name: schema_part_export_name(name, &slug),
            import_path: format!(
                "rpc/{}/{}",
                schema_part_slug(&kebab_case(name)),
                slug
            ),
            json: serde_json::to_string_pretty(&composite_part_schema(
                name,
                method,
                &ref_id,
                variant,
                definitions,
            )?)?,
            schema_path: format!("{name}/{slug}.json"),
        });
    }
    let root_object = root
        .as_object_mut()
        .context("composite root schema is not an object")?;
    root_object.remove("definitions");
    root_object.insert("oneOf".to_string(), Value::Array(root_refs));
    Ok(SchemaRender {
        root_json: serde_json::to_string_pretty(&root)?,
        refs,
    })
}

#[derive(Debug, Clone)]
struct SchemaRender {
    root_json: String,
    refs: Vec<SchemaRefEntry>,
}

fn composite_part_schema(
    root_name: &str,
    method: &str,
    ref_id: &str,
    variant: &Value,
    definitions: &serde_json::Map<String, Value>,
) -> Result<Value> {
    let mut object = serde_json::Map::new();
    object.insert(
        "$schema".to_string(),
        Value::String("http://json-schema.org/draft-07/schema#".to_string()),
    );
    object.insert("$id".to_string(), Value::String(ref_id.to_string()));
    object.insert(
        "title".to_string(),
        Value::String(format!("{root_name} {}", method)),
    );
    if let Some(variant_object) = variant.as_object() {
        for (key, value) in variant_object {
            object.insert(key.clone(), value.clone());
        }
    } else {
        bail!("composite variant for {root_name} is not an object");
    }
    let required_definitions = required_definitions(variant, definitions);
    if !required_definitions.is_empty() {
        object.insert("definitions".to_string(), Value::Object(required_definitions));
    }
    Ok(Value::Object(object))
}

fn required_definitions(
    value: &Value,
    definitions: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut pending = BTreeSet::new();
    collect_definition_refs(value, &mut pending);
    let mut seen = BTreeSet::new();
    let mut output = serde_json::Map::new();
    while let Some(name) = pending.iter().next().cloned() {
        pending.remove(&name);
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(definition) = definitions.get(&name) else {
            continue;
        };
        collect_definition_refs(definition, &mut pending);
        output.insert(name, definition.clone());
    }
    output
}

fn collect_definition_refs(value: &Value, refs: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|value| value.strip_prefix("#/definitions/"))
            {
                refs.insert(name.to_string());
            }
            for nested in object.values() {
                collect_definition_refs(nested, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_definition_refs(item, refs);
            }
        }
        _ => {}
    }
}

fn composite_method(variant: &Value) -> Result<&str> {
    variant
        .get("properties")
        .and_then(|value| value.get("method"))
        .and_then(|value| value.get("enum"))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .context("composite schema variant has no method enum")
}

fn schema_part_slug(value: &str) -> String {
    kebab_case(value)
        .replace(['/', '_'], "-")
        .trim_matches('-')
        .to_string()
}

fn schema_part_export_name(root_name: &str, slug: &str) -> String {
    format!("{}{}Schema", lower_camel(root_name), pascal_case(slug))
}

fn kebab_case(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-') {
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push('-');
        }
    }
    output
}

fn lower_camel(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_ascii_lowercase(), chars.collect::<String>())
}

fn pascal_case(value: &str) -> String {
    value
        .split(['-', '/', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_ascii_uppercase(), chars.collect::<String>())
        })
        .collect::<Vec<_>>()
        .join("")
}
