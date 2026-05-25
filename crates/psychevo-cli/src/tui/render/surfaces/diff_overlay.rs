#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn render_diff_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    overlay: &mut DiffOverlay,
    last_area: &mut Option<Rect>,
) {
    let overlay_area = centered_overlay_area(area);
    *last_area = Some(overlay_area);
    let theme = tui_theme();
    let block = Block::default()
        .title(format!(" {} ", overlay.title))
        .borders(Borders::ALL)
        .style(theme.menu_style())
        .border_style(theme.accent_style());
    let footer = Line::from(vec![
        Span::styled(" Esc ".to_string(), theme.accent_style()),
        Span::styled("close  ".to_string(), theme.dim_style()),
        Span::styled("Up/Down ".to_string(), theme.accent_style()),
        Span::styled("scroll  ".to_string(), theme.dim_style()),
        Span::styled("PgUp/PgDown ".to_string(), theme.accent_style()),
        Span::styled("page".to_string(), theme.dim_style()),
    ]);
    let footer_area = Rect {
        x: overlay_area.x.saturating_add(1),
        y: overlay_area
            .y
            .saturating_add(overlay_area.height.saturating_sub(1)),
        width: overlay_area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(
        Paragraph::new(Text::from(overlay.lines.clone()))
            .block(block)
            .scroll((overlay.scroll, 0))
            .wrap(Wrap { trim: false }),
        overlay_area,
    );
    frame.render_widget(
        Paragraph::new(footer).style(theme.menu_style()),
        footer_area,
    );
}

fn centered_overlay_area(area: Rect) -> Rect {
    let width = area.width.saturating_mul(9).saturating_div(10).max(40);
    let height = area.height.saturating_mul(4).saturating_div(5).max(8);
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
