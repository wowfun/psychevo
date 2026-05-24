#[allow(unused_imports)]
pub(crate) use super::*;

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn parses_basic_slash_commands() {
        assert_eq!(
            parse_slash_command("/help").unwrap(),
            Some(SlashCommand::Help)
        );
        assert!(parse_slash_command("/help now").is_err());
        assert_eq!(parse_slash_command("/q").unwrap(), Some(SlashCommand::Quit));
        assert_eq!(parse_slash_command("hello").unwrap(), None);
    }

    #[test]
    fn parses_session_and_model_commands() {
        assert_eq!(
            parse_slash_command("/sessions").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/resume").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/continue").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/stats").unwrap(),
            Some(SlashCommand::Usage)
        );
        assert_eq!(
            parse_slash_command("/usage").unwrap(),
            Some(SlashCommand::Usage)
        );
        assert_eq!(
            parse_slash_command("/context").unwrap(),
            Some(SlashCommand::Context)
        );
        assert_eq!(
            parse_slash_command("/refresh").unwrap(),
            Some(SlashCommand::Refresh)
        );
        assert_eq!(
            parse_slash_command("/reload-context").unwrap(),
            Some(SlashCommand::ReloadContextDeprecated)
        );
        assert_eq!(
            parse_slash_command("/btw").unwrap(),
            Some(SlashCommand::Btw(None))
        );
        assert_eq!(
            parse_slash_command("/btw explain this").unwrap(),
            Some(SlashCommand::Btw(Some("explain this".to_string())))
        );
        assert_eq!(
            parse_slash_command("/side explain this").unwrap(),
            Some(SlashCommand::Btw(Some("explain this".to_string())))
        );
        assert_eq!(
            parse_slash_command("/steer revise this").unwrap(),
            Some(SlashCommand::Steer("revise this".to_string()))
        );
        assert_eq!(
            parse_slash_command("/queue after this").unwrap(),
            Some(SlashCommand::Queue("after this".to_string()))
        );
        assert_eq!(
            parse_slash_command("/pending cancel").unwrap(),
            Some(SlashCommand::PendingCancel)
        );
        assert!(parse_slash_command("/steer").is_err());
        assert!(parse_slash_command("/queue").is_err());
        assert!(parse_slash_command("/pending").is_err());
        assert!(
            parse_slash_command("/session list")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /session")
        );
        assert!(parse_slash_command("/stats all").is_err());
        assert!(parse_slash_command("/context now").is_err());
        assert!(parse_slash_command("/session show abc").is_err());
        assert!(parse_slash_command("/session switch abc").is_err());
        assert_eq!(
            parse_slash_command("/model").unwrap(),
            Some(SlashCommand::ModelShow)
        );
        assert_eq!(
            parse_slash_command("/model -g").unwrap(),
            Some(SlashCommand::ModelShowScoped { global: true })
        );
        assert_eq!(
            parse_slash_command("/model --global").unwrap(),
            Some(SlashCommand::ModelShowScoped { global: true })
        );
        assert!(
            parse_slash_command("/model set mock/model")
                .unwrap_err()
                .to_string()
                .contains("usage: /model")
        );
        assert!(parse_slash_command("/model fetch").is_err());
        assert!(
            parse_slash_command("/models")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /models")
        );
    }

    #[test]
    fn validates_variants() {
        assert_eq!(
            parse_slash_command("/variant high").unwrap(),
            Some(SlashCommand::VariantSet("high".to_string()))
        );
        assert_eq!(
            parse_slash_command("/variant none").unwrap(),
            Some(SlashCommand::VariantSet("none".to_string()))
        );
        assert!(parse_slash_command("/variant").is_err());
        assert!(
            parse_slash_command("/variant set high")
                .unwrap_err()
                .to_string()
                .contains("usage: /variant <none|minimal|low|medium|high|xhigh|max>")
        );
        assert!(parse_slash_command("/effort high").is_err());
        assert!(parse_slash_command("/variant turbo").is_err());
    }

    #[test]
    fn parses_mode_commands() {
        assert_eq!(
            parse_slash_command("/mode plan").unwrap(),
            Some(SlashCommand::ModeSet("plan".to_string()))
        );
        assert_eq!(
            parse_slash_command("/mode default").unwrap(),
            Some(SlashCommand::ModeSet("default".to_string()))
        );
        assert!(parse_slash_command("/mode").is_err());
        assert!(
            parse_slash_command("/mode set plan")
                .unwrap_err()
                .to_string()
                .contains("usage: /mode <plan|default|acceptEdits|dontAsk|bypassPermissions>")
        );
        assert!(parse_slash_command("/mode build").is_err());
        assert!(parse_slash_command("/mode maybe").is_err());
    }

    #[test]
    fn parses_thinking_visibility_commands() {
        assert_eq!(
            parse_slash_command("/show-thinking").unwrap(),
            Some(SlashCommand::ThinkingToggle)
        );
        assert_eq!(
            parse_slash_command("/show-thinking on").unwrap(),
            Some(SlashCommand::ThinkingSet(true))
        );
        assert_eq!(
            parse_slash_command("/show-thinking off").unwrap(),
            Some(SlashCommand::ThinkingSet(false))
        );
        assert!(parse_slash_command("/show-thinking maybe").is_err());
        assert!(
            parse_slash_command("/thinking")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /thinking")
        );
    }

    #[test]
    fn parses_raw_visibility_and_copy_commands() {
        assert_eq!(
            parse_slash_command("/show-raw").unwrap(),
            Some(SlashCommand::RawToggle)
        );
        assert_eq!(
            parse_slash_command("/show-raw on").unwrap(),
            Some(SlashCommand::RawSet(true))
        );
        assert_eq!(
            parse_slash_command("/show-raw off").unwrap(),
            Some(SlashCommand::RawSet(false))
        );
        assert_eq!(
            parse_slash_command("/copy").unwrap(),
            Some(SlashCommand::Copy)
        );
        assert_eq!(
            parse_slash_command("/image \"image one.png\" describe").unwrap(),
            Some(SlashCommand::Image {
                source: "image one.png".to_string(),
                prompt: "describe".to_string(),
            })
        );
        assert!(parse_slash_command("/show-raw maybe").is_err());
        assert!(parse_slash_command("/copy now").is_err());
        assert!(parse_slash_command("/image").is_err());
        assert!(
            parse_slash_command("/raw")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /raw")
        );
    }

    #[test]
    fn parses_session_rename_command() {
        assert_eq!(
            parse_slash_command("/rename My session").unwrap(),
            Some(SlashCommand::Rename("My session".to_string()))
        );
        assert!(parse_slash_command("/rename").is_err());
    }

    #[test]
    fn parses_export_share_commands() {
        assert_eq!(
            parse_slash_command("/export").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: None,
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command(
                "/export out.json --format json --include messages,reasoning,provider-input-evidence,last-provider-request"
            )
            .unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse(
                    "messages,reasoning,provider-input-evidence,last-provider-request",
                    SessionArtifactKind::Export,
                )
                .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json --format json -i h,lpr").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse("h,lpr", SessionArtifactKind::Export)
                    .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command(r#"/export "session export.md" --format=markdown"#).unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("session export.md".to_string()),
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command(
                "/share share.md --include messages,reasoning,provider-input-evidence"
            )
            .unwrap(),
            Some(SlashCommand::Share(TuiShareOptions {
                path: Some("share.md".to_string()),
                include: SessionExportIncludeSet::parse(
                    "messages,reasoning,provider-input-evidence",
                    SessionArtifactKind::Share,
                )
                .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json -f json").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json -f=json").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command("/share share.md -i m,r,pie").unwrap(),
            Some(SlashCommand::Share(TuiShareOptions {
                path: Some("share.md".to_string()),
                include: SessionExportIncludeSet::parse("m,r,pie", SessionArtifactKind::Share)
                    .unwrap(),
            }))
        );
        assert!(parse_slash_command("/share --format json").is_err());
        assert!(parse_slash_command("/share -f json").is_err());
        assert!(parse_slash_command("/export --with-reasoning").is_err());
        assert!(parse_slash_command("/export --full-inputs").is_err());
        assert!(parse_slash_command("/export --last-request").is_err());
        assert!(parse_slash_command("/export --raw-requests").is_err());
        assert!(parse_slash_command("/share --with-reasoning").is_err());
        assert!(parse_slash_command("/share --full-inputs").is_err());
        assert!(parse_slash_command("/share --last-request").is_err());
        assert!(parse_slash_command("/share --raw-requests").is_err());
        assert!(parse_slash_command("/share --include last-provider-request").is_err());
        assert!(parse_slash_command("/export --format yaml").is_err());
        assert!(parse_slash_command("/export a b").is_err());
    }

    #[test]
    fn parses_skills_commands() {
        assert_eq!(
            parse_slash_command("/skills").unwrap(),
            Some(SlashCommand::Skills(None))
        );
        assert_eq!(
            parse_slash_command("/skills list").unwrap(),
            Some(SlashCommand::Skills(Some("list".to_string())))
        );
        assert_eq!(
            parse_slash_command("/reviewer extra context").unwrap(),
            Some(SlashCommand::SkillInvoke {
                name: "reviewer".to_string(),
                args: "extra context".to_string(),
            })
        );
        assert!(parse_slash_command("/skill:").is_err());
        assert!(parse_slash_command("/skill:reviewer").is_err());
    }

    #[test]
    fn configured_aliases_can_target_concrete_flagged_commands() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "slash_aliases": {
                    "/export -f json -i messages": ["/xj"]
                },
                "slash_keybinds": {
                    "/export -f json -i messages": "<leader>x"
                }
            }
        }))
        .expect("config");

        assert_eq!(
            parse_slash_command_with_config("/xj out.json", &config).unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse("messages", SessionArtifactKind::Export)
                    .unwrap(),
            }))
        );

        let items = configured_slash_menu_items(&config);
        let matches = slash_menu_items_from("/x", &items);
        assert_eq!(matches[0].command, "/xj");
        assert!(
            matches[0]
                .description
                .contains("alias for /export -f json -i messages")
        );
        assert_eq!(matches[0].completion, "/xj");
        assert_eq!(matches[0].replacement, "/xj");

        let help = format_slash_help_with_config(Some(0), &config);
        assert!(help.contains("/xj -> /export -f json -i messages"));
        assert!(help.contains("<leader>x -> /export -f json -i messages"));
        let sections = slash_help_sections_with_config(Some(0), &config);
        assert_eq!(
            sections.custom_commands,
            vec![
                "/export -f json -i messages - write session export (aliases: /xj) (shortcuts: <leader>x)"
                    .to_string()
            ]
        );
    }

    #[test]
    fn configured_custom_command_help_rows_are_deduped() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "slash_aliases": {
                    "/status": ["/st", "/state"]
                },
                "slash_keybinds": {
                    "/status": ["alt+s", "<leader>s"]
                }
            }
        }))
        .expect("config");

        let sections = slash_help_sections_with_config(Some(2), &config);
        let configured_rows = sections
            .custom_commands
            .iter()
            .filter(|row| row.starts_with("/status - "))
            .collect::<Vec<_>>();
        assert_eq!(configured_rows.len(), 1);
        assert!(configured_rows[0].contains("(aliases: /st, /state)"));
        assert!(configured_rows[0].contains("(shortcuts: alt+s, <leader>s)"));
    }

    #[test]
    fn configured_slash_keybinds_match_direct_and_leader_sequences() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "leader_key": "ctrl+x",
                "slash_keybinds": {
                    "/status": "alt+s",
                    "/model": "<leader>m",
                    "/usage": "none"
                }
            }
        }))
        .expect("config");

        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT), false),
            Some(SlashShortcutMatch::Command("/status".to_string()))
        );
        assert_eq!(
            config.shortcut_for_key(
                &KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                false
            ),
            Some(SlashShortcutMatch::LeaderPrefix)
        );
        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE), true),
            Some(SlashShortcutMatch::Command("/model".to_string()))
        );
        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT), false),
            None
        );
    }

    #[test]
    fn configured_slash_conflicts_are_rejected() {
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_aliases": { "/status": ["/model"] }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("conflicts with built-in command")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_aliases": { "/not-real": ["/nr"] }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("target")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": {
                        "/status": "alt+s",
                        "/model": "alt+s"
                    }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("duplicate slash shortcut")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "enter" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("fixed key")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "ctrl+a" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("fixed key")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("must include at least one shortcut")
        );
    }

    #[test]
    fn slash_menu_filters_and_marks_upcoming() {
        assert_eq!(slash_menu_items("/he")[0].command, "/help");
        assert_eq!(slash_menu_items("/usage")[0].command, "/usage");
        assert_eq!(slash_menu_items("/stats")[0].command, "/usage");
        assert_eq!(slash_prefix_menu_items("/stats")[0].command, "/usage");
        assert_eq!(slash_menu_items("/clear")[0].command, "/new");
        assert_eq!(slash_menu_items("/resume")[0].command, "/resume");
        assert_eq!(slash_menu_items("/refresh")[0].command, "/refresh");
        assert_eq!(slash_menu_items("/btw")[0].command, "/btw");
        assert!(slash_menu_items("/side").is_empty());
        assert!(slash_menu_items("/reload-context").is_empty());
        assert_eq!(slash_menu_items("/session").len(), 1);
        assert_eq!(slash_menu_items("/session")[0].command, "/sessions");
        assert!(slash_menu_items("/session ").is_empty());
        assert_eq!(slash_menu_items("/model").len(), 1);
        assert_eq!(slash_menu_items("/model")[0].command, "/model");
        assert_eq!(slash_menu_items("/model")[0].description, "configure model");
        let mode = slash_menu_items("/mode");
        assert_eq!(mode[0].command, "/mode");
        assert_eq!(mode[0].description, "set runtime permission mode");
        let variant = slash_menu_items("/var");
        assert_eq!(variant[0].command, "/variant");
        assert_eq!(variant[0].description, "set reasoning effort");
        let undo = slash_menu_items("/un");
        assert_eq!(undo[0].command, "/undo");
        assert!(!undo[0].upcoming);
        let rename = slash_menu_items("/ren");
        assert_eq!(rename[0].command, "/rename");
        assert_eq!(rename[0].description, "rename current session");
        let fuzzy_rename = slash_menu_items("/rn");
        assert_eq!(fuzzy_rename[0].command, "/rename");
        let fuzzy_model = slash_menu_items("/mdl");
        assert_eq!(fuzzy_model[0].command, "/model");
        assert!(slash_prefix_menu_items("/rn").is_empty());
        assert_eq!(
            parse_slash_command("/undo").unwrap(),
            Some(SlashCommand::Undo)
        );
        assert_eq!(
            parse_slash_command("/redo").unwrap(),
            Some(SlashCommand::Redo)
        );
        assert_eq!(
            parse_slash_command("/compact").unwrap(),
            Some(SlashCommand::Compact(None))
        );
        assert_eq!(
            parse_slash_command("/compact focus on todos").unwrap(),
            Some(SlashCommand::Compact(Some("focus on todos".to_string())))
        );
        assert!(parse_slash_command("/undo now").is_err());
        assert!(parse_slash_command("/redo now").is_err());
        assert_eq!(slash_menu_items("/compact")[0].command, "/compact");
        assert!(!slash_menu_items("/compact")[0].upcoming);
        assert_eq!(slash_menu_items("/copy")[0].command, "/copy");
        assert_eq!(slash_menu_items("/image")[0].command, "/image");
        assert_eq!(slash_menu_items("/show-raw")[0].command, "/show-raw");
    }

    #[test]
    fn formats_slash_help_from_registry() {
        let help = format_slash_help(Some(2));
        assert!(help.contains("General\n"));
        assert!(help.contains("\nCommands\n"));
        assert!(help.contains("\nCustom commands\n"));
        assert!(help.contains("Ctrl+B - toggle sidebar"));
        assert!(help.contains("Ctrl+O - copy latest answer as Markdown"));
        assert!(help.contains("/copy - copy latest answer as Markdown"));
        assert!(help.contains("/refresh - reload context and clean side sessions"));
        assert!(help.contains("/btw [prompt] - open a side conversation"));
        assert!(!help.contains("/reload-context"));
        assert!(!help.contains("/side"));
        assert!(help.contains("/usage - local usage and cost (aliases: /stats)"));
        assert!(help.contains("Reads persisted SQLite accounting and cost estimates"));
        assert!(help.contains("/sessions - list sessions"));
        assert!(help.contains("/resume [ref] - resume a previous session"));
        assert!(help.contains("/continue [ref] - continue a previous session"));
        assert!(help.contains("text surfaces list numbered sessions"));
        assert!(help.contains("Text surfaces accept latest, a listed number, an id prefix"));
        assert!(help.contains(
            "/export [path] [-f|--format markdown|json] [-i|--include list] - write session export"
        ));
        assert!(help.contains("last-provider-request can expose hidden prompts"));
        assert!(
            help.contains("/<skill-or-bundle> [args] - submit a skill or bundle (2 available)")
        );
        assert!(help.contains(
            "Submits the slash line while using the equivalent explicit skill or bundle marker"
        ));
        assert!(!help.contains("pevo run"));

        let empty = format_slash_help(Some(0));
        assert!(empty.contains("No custom commands available"));
    }

    #[test]
    fn slash_menu_can_filter_dynamic_skill_entries() {
        let mut items = base_slash_menu_items();
        items.push(SlashMenuItem {
            command: "/reviewer".to_string(),
            description: "Review code changes".to_string(),
            upcoming: false,
            aliases: Vec::new(),
            replacement: "/reviewer".to_string(),
            completion: "/reviewer".to_string(),
            configured_alias: false,
        });

        let matches = slash_menu_items_from("/rev", &items);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, "/reviewer");
        assert_eq!(matches[0].description, "Review code changes");
    }
}
