#[test]
fn adaptive_theme_falls_back_without_terminal_profile() {
    let theme = TuiTheme::from_profile(TerminalProfile::unknown());

    assert_eq!(theme.surface_bg, TUI_SURFACE_BG);
    assert_eq!(theme.selection_bg, TUI_SELECTION_BG);
    assert_eq!(theme.accent, TUI_CYAN);
}

#[test]
fn adaptive_theme_derives_distinct_light_and_dark_surfaces() {
    let dark = TuiTheme::from_profile(TerminalProfile::dark());
    let light = TuiTheme::from_profile(TerminalProfile::light());

    assert_ne!(dark.surface_bg, light.surface_bg);
    assert_ne!(dark.menu_bg, light.menu_bg);
    assert_ne!(light.selection_bg, TUI_SELECTION_BG);
    assert_ne!(light.accent, TUI_CYAN);
}

#[test]
fn adaptive_theme_ansi16_uses_stable_fallback_surfaces() {
    let profile = TerminalProfile {
        foreground: Some((238, 238, 238)),
        background: Some((12, 12, 14)),
        color_level: TerminalColorLevel::Ansi16,
    };
    let theme = TuiTheme::from_profile(profile);

    assert_eq!(theme.surface_bg, TUI_SURFACE_BG);
    assert_eq!(theme.menu_bg, Color::Rgb(16, 16, 20));
}

#[test]
fn terminal_probe_parses_default_color_replies() {
    let colors = parse_terminal_default_colors(
        b"\x1b]10;rgb:eeee/eeee/eeee\x1b\\\x1b]11;rgb:1010/1212/1414\x1b\\",
    )
    .expect("colors");

    assert_eq!(colors.foreground, (238, 238, 238));
    assert_eq!(colors.background, (16, 18, 20));
}

#[test]
fn motion_uses_static_activity_indicator_in_tests() {
    assert_eq!(activity_spinner_frame(Duration::from_secs(12)), "◦");
}

#[test]
fn markdown_renders_local_links_relative_to_workdir() {
    let temp = tempdir().expect("temp");
    let file = temp.path().join("src/main.rs");
    let markdown = format!(
        "See [ignored label]({}:42) and `inline`.",
        file.display()
    );

    let lines = render_markdown_lines(&markdown, temp.path());
    let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(text.contains("src/main.rs:42"), "{text}");
    assert!(!text.contains("ignored label"), "{text}");
    assert!(lines
        .iter()
        .flat_map(|line| &line.spans)
        .any(|span| span.style.fg == Some(tui_theme().code)));
}
