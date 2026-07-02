#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Clone, Default)]
pub struct ToolRouter {
    tools: Vec<Arc<dyn ToolBinding>>,
    by_name: BTreeMap<String, Arc<dyn ToolBinding>>,
    exposure_overrides: BTreeMap<String, ToolExposure>,
    tool_search: ToolSearchOptions,
}

const TOOL_SEARCH_NAME: &str = "tool_search";

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

    pub fn with_tool_search(mut self, options: ToolSearchOptions) -> Self {
        self.tool_search = options;
        self
    }

    pub fn tool(&self, name: &str) -> Option<Arc<dyn ToolBinding>> {
        self.by_name.get(name).cloned()
    }

    pub fn display_spec(&self, name: &str) -> ToolDisplaySpec {
        if name == TOOL_SEARCH_NAME && self.synthetic_tool_search_visible() {
            return ToolDisplaySpec::status();
        }
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
        let mut declarations = self
            .tools
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
            .collect::<Vec<_>>();
        if self.synthetic_tool_search_visible() {
            declarations.push(tool_search_declaration());
        }
        declarations
    }

    pub fn execute_tool_search(&mut self, args: &Value) -> ToolOutput {
        if !self.synthetic_tool_search_visible() {
            return ToolOutput::error("tool_search is not available");
        }
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let requested_limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(self.tool_search.default_limit);
        let limit = requested_limit
            .max(1)
            .min(self.tool_search.max_limit.max(1));
        let mut matches = self
            .tools
            .iter()
            .filter(|tool| self.effective_exposure(tool.name()) == Some(ToolExposure::Deferred))
            .filter_map(|tool| {
                let haystack = format!(
                    "{}\n{}\n{}",
                    tool.name(),
                    tool.description(),
                    serde_json::to_string(&tool.parameters()).unwrap_or_default()
                )
                .to_ascii_lowercase();
                let score = tool_search_score(&query, tool.name(), &haystack);
                (score > 0 || query.is_empty()).then(|| {
                    (
                        score,
                        tool.name().to_string(),
                        tool.description().to_string(),
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        matches.truncate(limit);
        let mut activated = Vec::new();
        for (_, name, _) in &matches {
            if self.activate_deferred(name) {
                activated.push(name.clone());
            }
        }
        let matches_json = matches
            .into_iter()
            .map(|(_, name, description)| {
                let is_activated = activated.contains(&name);
                json!({
                    "name": name,
                    "description": description,
                    "activated": is_activated,
                })
            })
            .collect::<Vec<_>>();
        ToolOutput::ok(json!({
            "query": query,
            "activated": activated,
            "matches": matches_json,
        }))
    }

    pub(crate) fn has_sequential_call(&self, tool_calls: &[ToolCallBlock]) -> bool {
        tool_calls.iter().any(|call| {
            if call.name == TOOL_SEARCH_NAME {
                return true;
            }
            self.tool(&call.name)
                .is_none_or(|tool| tool.execution_mode() == ToolExecutionMode::Sequential)
        })
    }

    pub(crate) fn is_tool_search_call(&self, name: &str) -> bool {
        name == TOOL_SEARCH_NAME && self.synthetic_tool_search_visible()
    }

    fn synthetic_tool_search_visible(&self) -> bool {
        self.tool_search.enabled
            && !self.by_name.contains_key(TOOL_SEARCH_NAME)
            && self
                .tools
                .iter()
                .any(|tool| self.effective_exposure(tool.name()) == Some(ToolExposure::Deferred))
    }
}

fn tool_search_declaration() -> ToolDeclaration {
    ToolDeclaration {
        name: TOOL_SEARCH_NAME.to_string(),
        description:
            "Search deferred tools by name, description, or schema and activate relevant matches for later tool calls."
                .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search terms for the deferred tools to activate."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of matching tools to activate."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }
}

fn tool_search_score(query: &str, name: &str, haystack: &str) -> usize {
    if query.is_empty() {
        return 1;
    }
    let mut score = 0usize;
    for term in query.split_whitespace() {
        if term.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case(term) {
            score += 8;
        } else if name.to_ascii_lowercase().contains(term) {
            score += 5;
        } else if haystack.contains(term) {
            score += 1;
        }
    }
    score
}
