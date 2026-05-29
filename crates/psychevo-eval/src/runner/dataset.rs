#[allow(unused_imports)]
use super::*;

pub(crate) fn import_dataset(request: DatasetImportRequest) -> Result<DatasetEntry> {
    let store = EvalStore::resolve(request.store_root)?;
    let input = resolve_cli_path(&request.path)?;
    let source = fs::canonicalize(&input)
        .with_context(|| format!("failed to resolve dataset path {}", input.display()))?;
    if !source.exists() {
        bail!("dataset path does not exist: {}", source.display());
    }

    let id = request.id.unwrap_or_else(|| {
        source
            .file_stem()
            .or_else(|| source.file_name())
            .and_then(|value| value.to_str())
            .map(slugify)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "dataset".to_string())
    });
    let id = slugify(&id);
    let dataset_dir = store.root.join("datasets").join(&id);
    fs::create_dir_all(&dataset_dir)
        .with_context(|| format!("failed to create {}", dataset_dir.display()))?;

    let payload_link = dataset_dir.join("payload");
    let payload = if link_dataset_payload(&source, &payload_link)? {
        PathBuf::from("payload")
    } else {
        source.clone()
    };
    let manifest = DatasetManifest {
        schema_version: INDEX_SCHEMA_VERSION,
        id: id.clone(),
        name: request.name.unwrap_or_else(|| id.clone()),
        kind: request.kind.unwrap_or_else(|| "local".to_string()),
        source: source.display().to_string(),
        payload,
        loader: request.loader,
        split: request.split,
        sample_limit: request.sample_limit,
        cache_key: request.cache_key,
        license: request.license,
        tags: request.tags,
        notes: request.notes,
    };
    write_toml_pretty(&dataset_dir.join("dataset.toml"), &manifest)?;
    store.refresh_after_dataset_change()?;
    read_dataset_entry(&dataset_dir.join("dataset.toml"))
}
