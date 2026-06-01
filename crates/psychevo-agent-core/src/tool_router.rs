#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Clone, Default)]
pub struct ToolRouter {
    tools: Vec<Arc<dyn ToolBinding>>,
    by_name: BTreeMap<String, Arc<dyn ToolBinding>>,
    exposure_overrides: BTreeMap<String, ToolExposure>,
}

impl ToolRouter {
    pub fn from_tools(tools: impl IntoIterator<Item = Arc<dyn ToolBinding>>) -> Self {
        let mut router = Self::default();
        for tool in tools {
            let name = tool.name().to_string();
            if router.by_name.contains_key(&name) {
                continue;
            }
            router.by_name.insert(name, Arc::clone(&tool));
            router.tools.push(tool);
        }
        router
    }

    pub fn tool(&self, name: &str) -> Option<Arc<dyn ToolBinding>> {
        self.by_name.get(name).cloned()
    }

    pub fn display_spec(&self, name: &str) -> ToolDisplaySpec {
        self.tool(name)
            .map(|tool| tool.display_spec())
            .unwrap_or_else(|| ToolDisplaySpec::for_name(name))
    }

    pub fn effective_exposure(&self, name: &str) -> Option<ToolExposure> {
        if let Some(exposure) = self.exposure_overrides.get(name).copied() {
            return Some(exposure);
        }
        self.tool(name).map(|tool| tool.exposure())
    }

    pub fn activate_deferred(&mut self, name: &str) -> bool {
        if self.effective_exposure(name) != Some(ToolExposure::Deferred) {
            return false;
        }
        self.exposure_overrides
            .insert(name.to_string(), ToolExposure::Direct);
        true
    }

    pub fn declarations(&self) -> Vec<ToolDeclaration> {
        self.tools
            .iter()
            .filter(|tool| {
                self.effective_exposure(tool.name())
                    .is_some_and(ToolExposure::is_model_visible)
            })
            .map(|tool| ToolDeclaration {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }

    pub(crate) fn has_sequential_call(&self, tool_calls: &[ToolCallBlock]) -> bool {
        tool_calls.iter().any(|call| {
            self.tool(&call.name)
                .is_none_or(|tool| tool.execution_mode() == ToolExecutionMode::Sequential)
        })
    }
}
