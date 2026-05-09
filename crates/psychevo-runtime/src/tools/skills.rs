#[derive(Clone)]
struct ListSkillsTool {
    options: SkillDiscoveryOptions,
}

impl ListSkillsTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for ListSkillsTool {
    fn name(&self) -> &str {
        "list_skills"
    }

    fn description(&self) -> &str {
        "List available skills with descriptions and locations."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"include_hidden":{"type":"boolean"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            let include_hidden = args
                .get("include_hidden")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match discover_skills(&options) {
                Ok(catalog) => ToolOutput::ok(list_skills_value(&catalog, include_hidden)),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

#[derive(Clone)]
struct ViewSkillTool {
    options: SkillDiscoveryOptions,
}

impl ViewSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for ViewSkillTool {
    fn name(&self) -> &str {
        "view_skill"
    }

    fn description(&self) -> &str {
        "Load a skill's instructions or a supporting file within the skill directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"file_path":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match view_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn view_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let catalog = discover_skills(options)?;
    let name = required_string(&args, "name")?;
    let file_path = optional_string(&args, "file_path")?;
    view_skill_value(&catalog, name, file_path)
}

#[derive(Clone)]
struct CreateSkillTool {
    options: SkillDiscoveryOptions,
}

impl CreateSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for CreateSkillTool {
    fn name(&self) -> &str {
        "create_skill"
    }

    fn description(&self) -> &str {
        "Create a new local skill in the global or project skills directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name","description"],"properties":{"name":{"type":"string"},"description":{"type":"string"},"scope":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match create_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn create_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let name = required_string(&args, "name")?;
    let description = required_string(&args, "description")?;
    create_skill(
        &options.home,
        &options.workdir,
        target_from_args(&args)?,
        name,
        description,
    )
}

#[derive(Clone)]
struct PatchSkillTool {
    options: SkillDiscoveryOptions,
}

impl PatchSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for PatchSkillTool {
    fn name(&self) -> &str {
        "patch_skill"
    }

    fn description(&self) -> &str {
        "Patch a mutable local skill by replacing one exact text occurrence."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name","old","new"],"properties":{"name":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match patch_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn patch_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let catalog = discover_skills(options)?;
    let name = required_string(&args, "name")?;
    let old = required_string(&args, "old")?;
    let new = required_string(&args, "new")?;
    patch_skill(&catalog, &options.home, &options.workdir, name, old, new)
}

#[derive(Clone)]
struct RemoveSkillTool {
    options: SkillDiscoveryOptions,
}

impl RemoveSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for RemoveSkillTool {
    fn name(&self) -> &str {
        "remove_skill"
    }

    fn description(&self) -> &str {
        "Remove a mutable local skill."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match remove_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn remove_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let catalog = discover_skills(options)?;
    let name = required_string(&args, "name")?;
    remove_skill(&catalog, &options.home, &options.workdir, name)
}

#[derive(Clone)]
struct EnableSkillTool {
    options: SkillDiscoveryOptions,
}

impl EnableSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for EnableSkillTool {
    fn name(&self) -> &str {
        "enable_skill"
    }

    fn description(&self) -> &str {
        "Enable a skill name in the global or project skills directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"scope":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match set_enabled_tool_impl(&options, args, true) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

#[derive(Clone)]
struct DisableSkillTool {
    options: SkillDiscoveryOptions,
}

impl DisableSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for DisableSkillTool {
    fn name(&self) -> &str {
        "disable_skill"
    }

    fn description(&self) -> &str {
        "Disable a skill name in the global or project skills directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"scope":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match set_enabled_tool_impl(&options, args, false) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn set_enabled_tool_impl(
    options: &SkillDiscoveryOptions,
    args: Value,
    enabled: bool,
) -> Result<Value> {
    let name = required_string(&args, "name")?;
    set_skill_enabled(
        &options.home,
        &options.workdir,
        target_from_args(&args)?,
        name,
        enabled,
    )
}

#[derive(Clone)]
struct InstallSkillTool {
    options: SkillDiscoveryOptions,
}

impl InstallSkillTool {
    fn new(options: SkillDiscoveryOptions) -> Self {
        Self { options }
    }
}

impl ToolBinding for InstallSkillTool {
    fn name(&self) -> &str {
        "install_skill"
    }

    fn description(&self) -> &str {
        "Install a local or Git skill into the global or project skills directory. Dangerous scanner verdicts cannot be forced through this tool."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["source"],"properties":{"source":{"type":"string"},"name":{"type":"string"},"all":{"type":"boolean"},"scope":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let options = self.options.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match install_skill_tool_impl(&options, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn install_skill_tool_impl(options: &SkillDiscoveryOptions, args: Value) -> Result<Value> {
    let source = required_string(&args, "source")?.to_string();
    let name = optional_string(&args, "name")?.map(ToOwned::to_owned);
    let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
    install_skill(
        &options.home,
        &options.workdir,
        InstallOptions {
            source,
            target: target_from_args(&args)?,
            name,
            all,
            force: false,
        },
    )
}

fn target_from_args(args: &Value) -> Result<SkillTarget> {
    match optional_string(args, "scope")?.unwrap_or("global") {
        "global" => Ok(SkillTarget::Global),
        "project" => Ok(SkillTarget::Project),
        other => Err(Error::Message(format!(
            "scope must be global or project, got {other}"
        ))),
    }
}
