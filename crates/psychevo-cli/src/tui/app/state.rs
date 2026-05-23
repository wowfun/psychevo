struct TuiApp {
    env_map: BTreeMap<String, String>,
    home: PathBuf,
    state_path: PathBuf,
    state: TuiState,
    db_path: PathBuf,
    config_path: Option<PathBuf>,
    workdir: PathBuf,
    workdir_key: String,
    current_session: Option<String>,
    current_session_title: Option<String>,
    force_new_once: bool,
    current_model: Option<String>,
    current_variant: Option<String>,
    selected_model: Option<ConfiguredModel>,
    current_mode: RunMode,
    current_permission_mode: PermissionMode,
    startup_agent: Option<String>,
    current_agent: Option<String>,
    current_agent_explicit_default: bool,
    no_agents: bool,
    no_skills: bool,
    skill_inputs: Vec<String>,
    thinking_visible: bool,
    raw_visible: bool,
    clipboard: ClipboardSink,
    renderer: TuiRenderer,
    debug: bool,
    had_error: bool,
    last_context_snapshot: Option<ContextSnapshot>,
    model_catalog: ModelCatalogCache,
    clipboard_result_tx: std::sync::mpsc::Sender<Result<(), String>>,
    clipboard_result_rx: std::sync::mpsc::Receiver<Result<(), String>>,
    clipboard_copies_in_flight: usize,
    slash_config: EffectiveSlashConfig,
    btw_side: Option<BtwSideState>,
    side_cleanup_task: Option<SideCleanupTask>,
    compaction_task: Option<CompactionTask>,
}

struct BtwSideState {
    parent_session: String,
    parent_session_title: Option<String>,
    parent_model: Option<String>,
    parent_variant: Option<String>,
    parent_mode: RunMode,
    parent_permission_mode: PermissionMode,
    parent_agent: Option<String>,
    parent_agent_explicit_default: bool,
    side_session: String,
}

struct SideCleanupTask {
    task: JoinHandle<std::result::Result<usize, String>>,
}

struct CompactionTask {
    session_id: String,
    command_echo: Option<String>,
    manual: bool,
    task: JoinHandle<std::result::Result<CompactionResult, String>>,
}

impl TuiApp {
    fn current_skill_catalog(&self) -> Option<SkillCatalog> {
        discover_skills(&SkillDiscoveryOptions {
            home: self.home.clone(),
            workdir: self.workdir.clone(),
            config_path: self.config_path.clone(),
            env: self.env_map.clone(),
            explicit_inputs: self.skill_inputs.clone(),
            no_skills: self.no_skills,
        })
        .ok()
    }

    fn current_agent_catalog(&self) -> Option<AgentCatalog> {
        if self.no_agents {
            return None;
        }
        discover_agents(&AgentDiscoveryOptions {
            home: self.home.clone(),
            workdir: self.workdir.clone(),
            env: self.env_map.clone(),
            explicit_inputs: self.current_agent.iter().cloned().collect(),
            no_agents: self.no_agents,
        })
        .ok()
    }

    fn current_skill_bundles(&self) -> Vec<SkillBundle> {
        if self.no_skills {
            return Vec::new();
        }
        list_skill_bundles(&self.home, &self.workdir).unwrap_or_default()
    }

    fn slash_items(&self) -> Vec<SlashMenuItem> {
        let mut items = configured_slash_menu_items(&self.slash_config);
        let mut dynamic_names = BTreeSet::new();
        for bundle in self.current_skill_bundles() {
            let command = format!("/{}", bundle.slug);
            dynamic_names.insert(bundle.slug.clone());
            items.push(SlashMenuItem {
                command: command.clone(),
                description: bundle.description,
                upcoming: false,
                aliases: Vec::new(),
                replacement: command.clone(),
                completion: command,
                configured_alias: false,
            });
        }
        if let Some(catalog) = self.current_skill_catalog() {
            for skill in catalog.skills {
                if dynamic_names.contains(&skill.name) {
                    continue;
                }
                let command = format!("/{}", skill.name);
                items.push(SlashMenuItem {
                    command: command.clone(),
                    description: skill.description,
                    upcoming: false,
                    aliases: Vec::new(),
                    replacement: command.clone(),
                    completion: command,
                    configured_alias: false,
                });
            }
        }
        items
    }

    fn slash_menu_items(&self, input: &str) -> Vec<SlashMenuItem> {
        slash_menu_items_from(input, &self.slash_items())
    }

    fn skill_search_matches(&self, query: &str) -> Vec<SkillSearchMatch> {
        let Some(catalog) = self.current_skill_catalog() else {
            return Vec::new();
        };
        let query = query.trim().to_lowercase();
        let mut matches = catalog
            .skills
            .into_iter()
            .filter_map(|skill| {
                skill_match_rank(&skill.name, &skill.description, &query).map(|rank| {
                    (
                        rank,
                        SkillSearchMatch {
                            name: skill.name,
                            description: skill.description,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        matches
            .into_iter()
            .take(FILE_POPUP_MAX_ROWS)
            .map(|(_, entry)| entry)
            .collect()
    }

    fn agent_search_matches(&self, query: &str) -> Vec<AgentSearchMatch> {
        let query = query.trim().to_lowercase();
        let Some(catalog) = self.current_agent_catalog() else {
            return Vec::new();
        };
        let mut matches = catalog
            .agents
            .into_iter()
            .filter_map(|agent| {
                skill_match_rank(&agent.name, &agent.description, &query).map(|rank| {
                    (
                        rank,
                        AgentSearchMatch {
                            name: agent.name,
                            description: agent.description,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        matches
            .into_iter()
            .take(FILE_POPUP_MAX_ROWS)
            .map(|(_, entry)| entry)
            .collect()
    }

    fn sync_agent_popup(&self, ui: &mut FullscreenUi<'_>) {
        if ui.shell_mode || ui.current_skill_token().is_some() {
            ui.close_agent_popup();
            return;
        }
        let Some(token) = ui.current_agent_token() else {
            ui.sync_agent_popup(Vec::new());
            return;
        };
        ui.sync_agent_popup(self.agent_search_matches(&token.query));
        if ui.agent_popup_visible() {
            ui.close_file_popup();
        }
    }

    fn sync_skill_popup(&self, ui: &mut FullscreenUi<'_>) {
        if ui.shell_mode || ui.current_file_token().is_some() || ui.agent_popup_visible() {
            ui.close_skill_popup();
            return;
        }
        let Some(token) = ui.current_skill_token() else {
            ui.sync_skill_popup(Vec::new());
            return;
        };
        ui.sync_skill_popup(self.skill_search_matches(&token.query));
    }
}

fn skill_match_rank(name: &str, description: &str, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((3, 0));
    }
    let name_lower = name.to_lowercase();
    let description_lower = description.to_lowercase();
    if name_lower.starts_with(query) {
        return Some((0, name_lower.len().saturating_sub(query.len())));
    }
    if let Some(index) = name_lower.find(query) {
        return Some((1, index));
    }
    if let Some(score) = fuzzy_subsequence_score(&name_lower, query) {
        return Some((2, score));
    }
    description_lower.find(query).map(|index| (4, index))
}
