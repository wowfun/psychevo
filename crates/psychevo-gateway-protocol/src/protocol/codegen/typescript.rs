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
