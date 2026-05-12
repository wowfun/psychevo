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

    let lines = render_markdown_lines(&markdown, temp.path(), Some(80));
    let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(text.contains("src/main.rs:42"), "{text}");
    assert!(!text.contains("ignored label"), "{text}");
    assert!(lines
        .iter()
        .flat_map(|line| &line.spans)
        .any(|span| span.style.fg == Some(tui_theme().code)));
}

#[test]
fn markdown_renders_tables_as_boxes_with_pipe_fallback() {
    let temp = tempdir().expect("temp");
    let markdown = "| Name | Value |\n|---|---:|\n| alpha | 42 |";

    let wide = render_markdown_lines(markdown, temp.path(), Some(80));
    let wide_text = wide.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(wide_text.contains("┌"), "{wide_text}");
    assert!(wide_text.contains("│ Name"), "{wide_text}");
    assert!(wide_text.contains("42"), "{wide_text}");

    let narrow = render_markdown_lines(markdown, temp.path(), Some(12));
    let narrow_text = narrow.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(narrow_text.contains("| Name | Value |"), "{narrow_text}");
    assert!(!narrow_text.contains("┌"), "{narrow_text}");
}

#[test]
fn markdown_unwraps_fenced_markdown_tables_only_when_table_like() {
    let temp = tempdir().expect("temp");
    let table_fence = "```markdown\n| Name | Value |\n|---|---|\n| alpha | beta |\n```";
    let rich = render_markdown_lines(table_fence, temp.path(), Some(80));
    let rich_text = rich.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rich_text.contains("┌"), "{rich_text}");
    assert!(!rich_text.contains("```"), "{rich_text}");

    let ordinary_fence = "```markdown\n# Title\nnot a table\n```";
    let code = render_markdown_lines(ordinary_fence, temp.path(), Some(80));
    let code_text = code.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(code_text.contains("╭─ code markdown"), "{code_text}");
    assert!(code_text.contains("# Title"), "{code_text}");
}

#[test]
fn markdown_code_blocks_have_boundaries_folding_and_highlighting() {
    let temp = tempdir().expect("temp");
    let code = (1..=10)
        .map(|index| format!("fn line_{index}() {{ let value = \"{index}\"; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let markdown = format!("```rust\n{code}\n```");

    let lines = render_markdown_lines(&markdown, temp.path(), Some(100));
    let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(text.contains("╭─ code rust"), "{text}");
    assert!(text.contains("╰─"), "{text}");
    assert!(text.contains("... 2 more lines"), "{text}");
    assert!(lines.iter().flat_map(|line| &line.spans).any(|span| {
        span.content.as_ref() == "fn" && span.style.fg == Some(tui_theme().accent)
    }));
    assert!(lines.iter().flat_map(|line| &line.spans).any(|span| {
        span.content.as_ref() == "\"1\"" && span.style.fg == Some(tui_theme().success)
    }));
}

#[test]
fn markdown_exposes_normal_link_destinations() {
    let temp = tempdir().expect("temp");
    let lines = render_markdown_lines(
        "See [docs](https://example.test/docs).",
        temp.path(),
        Some(80),
    );
    let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(text.contains("docs (https://example.test/docs)"), "{text}");
}

#[test]
fn raw_answer_display_keeps_markdown_source_instead_of_rich_projection() {
    let temp = tempdir().expect("temp");
    let row = TranscriptRow::with_title(
        TranscriptKind::Answer,
        "",
        "# Title\n\n```rust\nfn main() {}\n```".to_string(),
    );

    let rich = answer_lines(&row, false, true, 80, temp.path(), false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    let raw = answer_lines(&row, false, true, 80, temp.path(), true)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rich.contains("╭─ code rust"), "{rich}");
    assert!(!rich.contains("```rust"), "{rich}");
    assert!(raw.contains("```rust"), "{raw}");
    assert!(raw.contains("fn main() {}"), "{raw}");
    assert!(!raw.contains("╭─ code rust"), "{raw}");
}
