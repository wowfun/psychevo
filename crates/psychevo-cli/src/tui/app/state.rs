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

    fn slash_items(&self) -> Vec<SlashMenuItem> {
        let mut items = base_slash_menu_items();
        if let Some(catalog) = self.current_skill_catalog() {
            for skill in catalog.skills {
                items.push(SlashMenuItem {
                    command: format!("/skill:{}", skill.name),
                    description: skill.description,
                    upcoming: false,
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

    fn sync_skill_popup(&self, ui: &mut FullscreenUi<'_>) {
        if ui.current_file_token().is_some() {
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
