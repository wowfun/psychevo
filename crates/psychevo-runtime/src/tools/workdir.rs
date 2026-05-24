#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Clone)]
pub(crate) struct WorkdirTool {
    pub(crate) workdir: PathBuf,
    pub(crate) context: ToolRuntimeContext,
}

impl WorkdirTool {
    #[cfg(test)]
    pub(crate) fn new(workdir: PathBuf) -> Self {
        Self::with_context(workdir, ToolRuntimeContext::default())
    }

    pub(crate) fn with_context(workdir: PathBuf, context: ToolRuntimeContext) -> Self {
        Self { workdir, context }
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.context.task_id
    }

    pub(crate) fn lsp_config(&self) -> &LspConfig {
        &self.context.lsp
    }

    pub(crate) fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub(crate) fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        let target = self.resolve_raw(raw);
        let canonical = target.canonicalize()?;
        self.ensure_contained(&canonical)?;
        Ok(canonical)
    }

    pub(crate) fn resolve_write_target(&self, raw: &str) -> Result<(PathBuf, bool)> {
        let target = self.resolve_raw(raw);
        if target.exists() {
            let canonical = target.canonicalize()?;
            self.ensure_contained(&canonical)?;
            return Ok((canonical, false));
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::Message("target has no parent".to_string()))?
            .to_path_buf();
        let mut existing = parent.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| Error::Message("no existing parent under workdir".to_string()))?;
        }
        let canonical_parent = existing.canonicalize()?;
        self.ensure_contained(&canonical_parent)?;
        let dirs_created = !parent.exists();
        Ok((target, dirs_created))
    }

    pub(crate) fn resolve_raw(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workdir.join(path)
        }
    }

    pub(crate) fn ensure_contained(&self, path: &Path) -> Result<()> {
        if path == self.workdir || path.starts_with(&self.workdir) {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "path escapes workdir: {}",
                path.display()
            )))
        }
    }

    pub(crate) fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.workdir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}
