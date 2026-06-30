#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Clone)]
pub(crate) struct CwdTool {
    pub(crate) cwd: PathBuf,
    pub(crate) context: ToolRuntimeContext,
}

impl CwdTool {
    #[cfg(test)]
    pub(crate) fn new(cwd: PathBuf) -> Self {
        Self::with_context(cwd, ToolRuntimeContext::default())
    }

    pub(crate) fn with_context(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self { cwd, context }
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.context.task_id
    }

    pub(crate) fn lsp_config(&self) -> &LspConfig {
        &self.context.lsp
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn sandbox_policy(&self) -> &SandboxPolicy {
        &self.context.sandbox_policy
    }

    pub(crate) fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        let target = self.resolve_raw(raw)?;
        Ok(target.canonicalize()?)
    }

    pub(crate) fn resolve_write_target(&self, raw: &str) -> Result<(PathBuf, bool)> {
        let target = self.resolve_raw(raw)?;
        if target.exists() {
            return Ok((target.canonicalize()?, false));
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::Message("target has no parent".to_string()))?
            .to_path_buf();
        let mut existing = parent.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| Error::Message("no existing parent for target".to_string()))?;
        }
        let dirs_created = !parent.exists();
        Ok((target, dirs_created))
    }

    pub(crate) fn resolve_raw(&self, raw: &str) -> Result<PathBuf> {
        crate::host_paths::resolve_input_path(raw, &self.cwd)
    }

    pub(crate) fn ensure_contained(&self, path: &Path) -> Result<()> {
        if path == self.cwd || path.starts_with(&self.cwd) {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "path escapes cwd: {}",
                path.display()
            )))
        }
    }

    pub(crate) fn ensure_sandbox_write_allowed(
        &self,
        path: &Path,
        tool_call_id: Option<&str>,
    ) -> Result<()> {
        match self.sandbox_policy().ensure_write_allowed(path) {
            Ok(()) => Ok(()),
            Err(err) => {
                if let Some(tool_call_id) = tool_call_id
                    && self
                        .context
                        .sandbox_grants
                        .allows_once(tool_call_id, path)?
                {
                    return Ok(());
                }
                Err(err)
            }
        }
    }

    pub(crate) fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.cwd)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}
