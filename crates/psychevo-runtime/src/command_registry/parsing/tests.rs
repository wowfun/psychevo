#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn slash_registry_keeps_aliases_hidden_from_canonical_rows() {
        assert!(SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/help"));
        assert!(SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/btw"));
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/refresh")
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/side"));
        assert!(
            !SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/reload-context")
        );
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/usage" && spec.aliases == ["/stats"].as_slice())
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/stats"));
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/resume")
        );
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/continue")
        );
        assert!(
            !SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/thinking")
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/raw"));
    }

    #[test]
    fn cli_registry_records_process_commands() {
        let names = CLI_COMMANDS
            .iter()
            .map(|spec| spec.canonical)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "init", "run", "tui", "serve", "gateway", "acp", "mcp", "profile", "agent",
                "skill", "tool", "session", "model", "config", "auth", "stats", "context",
            ]
        );
        assert!(
            CLI_COMMANDS
                .iter()
                .all(|spec| spec.surface == CommandSurface::PevoCli)
        );
    }

    #[test]
    fn shared_parser_resolves_aliases_and_unknowns() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line(" /stats ") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.canonical, "/usage");
        assert_eq!(invocation.command, "/stats");

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/commands") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.canonical, "/help");

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/history") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.canonical, "/sessions");

        let SlashCommandParse::Unknown { command, args, .. } =
            parse_slash_command_line("/made-up hello")
        else {
            panic!("expected unknown command");
        };
        assert_eq!(command, "/made-up");
        assert_eq!(args, "hello");
        assert_eq!(
            parse_slash_command_line("hello"),
            SlashCommandParse::NotSlash
        );
    }

    #[test]
    fn acp_capabilities_hide_tui_only_commands() {
        let available = available_slash_commands_for_surface(
            &[
                CommandCapability::ActiveTurnControl,
                CommandCapability::Queue,
                CommandCapability::SessionSwitch,
                CommandCapability::SessionRevert,
                CommandCapability::ArtifactWrite,
                CommandCapability::WorkspaceDiff,
                CommandCapability::ConfigWrite,
                CommandCapability::PolicyWrite,
                CommandCapability::SkillStateWrite,
            ],
            false,
            &[],
            100,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"tools"));
        assert!(names.contains(&"diff"));
        assert!(names.contains(&"resume"));
        assert!(names.contains(&"undo"));
        assert!(names.contains(&"redo"));
        assert!(!names.contains(&"copy"));
        assert!(!names.contains(&"image"));
        assert!(!names.contains(&"quit"));
    }

    #[test]
    fn active_turn_filters_disruptive_commands() {
        let available = available_slash_commands_for_surface(
            &[
                CommandCapability::ActiveTurnControl,
                CommandCapability::Queue,
                CommandCapability::SessionSwitch,
                CommandCapability::SessionRevert,
                CommandCapability::ArtifactWrite,
                CommandCapability::WorkspaceDiff,
            ],
            true,
            &[],
            100,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"steer"));
        assert!(names.contains(&"queue"));
        assert!(names.contains(&"diff"));
        assert!(!names.contains(&"resume"));
        assert!(!names.contains(&"undo"));
        assert!(!names.contains(&"redo"));
        assert!(!names.contains(&"compact"));
    }

    #[test]
    fn available_commands_include_presentation_metadata() {
        let dynamic = [DynamicSlashCommand {
            name: "x-daily".to_string(),
            summary: "Fetch X daily posts.".to_string(),
            prompt: "$x-daily ".to_string(),
        }];
        let available = available_slash_commands_for_surface(
            &[
                CommandCapability::SessionSwitch,
                CommandCapability::ArtifactWrite,
                CommandCapability::WorkspaceDiff,
            ],
            false,
            &dynamic,
            100,
        );

        let diff = available
            .commands
            .iter()
            .find(|command| command.name == "diff")
            .expect("diff command");
        assert_eq!(diff.action, SlashCommandAction::Diff);
        assert_eq!(diff.presentation.kind, CommandPresentationKind::Inspect);
        assert_eq!(diff.presentation.destination, CommandDestination::Preview);

        let sessions = available
            .commands
            .iter()
            .find(|command| command.name == "sessions")
            .expect("sessions command");
        assert_eq!(
            sessions.presentation.kind,
            CommandPresentationKind::Navigate
        );
        assert_eq!(
            sessions.presentation.destination,
            CommandDestination::History
        );

        let dynamic = available
            .commands
            .iter()
            .find(|command| command.name == "x-daily")
            .expect("dynamic command");
        assert_eq!(dynamic.action, SlashCommandAction::SkillInvoke);
        assert_eq!(
            dynamic.presentation.kind,
            CommandPresentationKind::Extension
        );
        assert_eq!(
            dynamic.presentation.destination,
            CommandDestination::Composer
        );
    }

    #[test]
    fn undo_redo_require_session_revert_capability() {
        let without_revert = available_slash_commands_for_surface(
            &[
                CommandCapability::SessionSwitch,
                CommandCapability::ArtifactWrite,
                CommandCapability::WorkspaceDiff,
            ],
            false,
            &[],
            100,
        );
        let names = without_revert
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(!names.contains(&"undo"));
        assert!(!names.contains(&"redo"));

        let with_revert = available_slash_commands_for_surface(
            &[CommandCapability::SessionRevert],
            false,
            &[],
            100,
        );
        let names = with_revert
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"undo"));
        assert!(names.contains(&"redo"));
    }

    #[test]
    fn shared_effect_parses_diff_and_allows_active_turn() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/diff") else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[CommandCapability::WorkspaceDiff],
            SlashCommandSurface::Acp,
            true,
        )
        .expect("diff effect");
        assert_eq!(effect, SlashCommandEffect::Diff);
    }

    #[test]
    fn shared_effect_parses_btw_when_side_conversations_are_supported() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/btw explain this")
        else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[CommandCapability::SideConversation],
            SlashCommandSurface::WebDesktop,
            true,
        )
        .expect("btw effect");
        assert_eq!(
            effect,
            SlashCommandEffect::Btw {
                prompt: Some("explain this".to_string()),
            }
        );
    }

    #[test]
    fn shared_effect_parses_mission_team_and_goal() {
        let SlashCommandParse::Known(invocation) =
            parse_slash_command_line("/mission --team ship implement the feature")
        else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[],
            SlashCommandSurface::WebDesktop,
            false,
        )
        .expect("mission effect");
        let SlashCommandEffect::Mission { prompt, team, goal } = effect else {
            panic!("expected mission effect");
        };
        assert_eq!(team.as_deref(), Some("ship"));
        assert_eq!(goal, "implement the feature");
        assert!(prompt.contains("Team template: ship"));
        assert!(prompt.contains("implement the feature"));
    }

    #[test]
    fn shared_effect_parses_voice_modes() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/voice tts") else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[],
            SlashCommandSurface::Messaging,
            true,
        )
        .expect("voice effect");
        assert_eq!(effect, SlashCommandEffect::Voice("tts".to_string()));

        let SlashCommandParse::Known(invalid) = parse_slash_command_line("/voice loud") else {
            panic!("expected known command");
        };
        let err = slash_invocation_effect(&invalid, &[], SlashCommandSurface::Messaging, false)
            .expect_err("invalid voice mode");
        assert_eq!(err, "usage: /voice <on|tts|off|status>");
    }

    #[test]
    fn web_desktop_surface_returns_surface_specific_guidance() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/image ./a.png")
        else {
            panic!("expected known command");
        };
        let effect =
            slash_invocation_effect(&invocation, &[], SlashCommandSurface::WebDesktop, false)
                .expect("unsupported effect");
        assert_eq!(
            effect,
            SlashCommandEffect::Unsupported(
                "/image is not advertised here; attach images with the Web/Desktop shell's native attachment flow when available.".to_string(),
            )
        );
    }

    #[test]
    fn dynamic_commands_are_capped_after_core_commands() {
        let dynamic = (0..5)
            .map(|idx| DynamicSlashCommand {
                name: format!("skill-{idx}"),
                summary: "skill".to_string(),
                prompt: format!("$skill-{idx} "),
            })
            .collect::<Vec<_>>();
        let available = available_slash_commands_for_surface(&[], false, &dynamic, 3);
        assert_eq!(available.commands.len(), 3);
        assert_eq!(available.hidden_dynamic, 5);
    }

    #[test]
    fn shared_effect_parses_text_forms() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/tools enable web")
        else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[CommandCapability::ConfigWrite],
            SlashCommandSurface::Acp,
            false,
        )
        .expect("effect");
        assert_eq!(
            effect,
            SlashCommandEffect::ToolsetSet {
                name: "web".to_string(),
                enabled: true,
            }
        );
    }

    #[test]
    fn shared_effect_parses_model_scope_flags_and_rejects_set_form() {
        let SlashCommandParse::Known(local) = parse_slash_command_line("/model --local") else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &local,
            &[CommandCapability::ConfigWrite],
            SlashCommandSurface::Acp,
            false,
        )
        .expect("local model effect");
        assert_eq!(effect, SlashCommandEffect::ShowModel);

        let SlashCommandParse::Known(global) = parse_slash_command_line("/model -g") else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &global,
            &[CommandCapability::ConfigWrite],
            SlashCommandSurface::Acp,
            false,
        )
        .expect("global model effect");
        assert_eq!(effect, SlashCommandEffect::ShowModel);

        let SlashCommandParse::Known(old_set) = parse_slash_command_line("/model set mock/model")
        else {
            panic!("expected known command");
        };
        let err = slash_invocation_effect(
            &old_set,
            &[CommandCapability::ConfigWrite],
            SlashCommandSurface::Acp,
            false,
        )
        .expect_err("old set form rejected");
        assert!(err.contains("usage: /model [--local|-g|--global]"));
    }
}
