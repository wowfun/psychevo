#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) trait TuiRenderable {
    fn desired_height(&self, width: u16) -> u16;
    fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DisplayRowTone {
    #[default]
    Normal,
    Accent,
    Identity,
    Dim,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DisplayRow {
    pub(crate) marker: &'static str,
    pub(crate) label: String,
    pub(crate) description: Option<String>,
    pub(crate) meta: Option<String>,
    pub(crate) selected: bool,
    pub(crate) disabled: bool,
    pub(crate) tone: DisplayRowTone,
}

impl DisplayRow {
    pub(crate) fn line(&self, width: u16) -> Line<'static> {
        let theme = tui_theme();
        let base = if self.selected {
            theme.selected_row_style()
        } else {
            Style::default()
        };
        let label_style = match self.tone {
            DisplayRowTone::Normal => base,
            DisplayRowTone::Accent => base.fg(theme.accent),
            DisplayRowTone::Identity => base.fg(theme.identity),
            DisplayRowTone::Dim => base.fg(theme.dim),
        };
        let label_style = if self.disabled {
            label_style.fg(theme.dim)
        } else {
            label_style
        };
        let marker = if self.selected { "› " } else { self.marker };
        let mut spans = vec![
            Span::styled(marker.to_string(), base.fg(theme.accent)),
            Span::styled(self.label.clone(), label_style),
        ];
        if let Some(description) = &self.description {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(description.clone(), base.fg(theme.dim)));
        }
        if let Some(meta) = &self.meta {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(meta.clone(), base.fg(theme.dim)));
        }
        truncate_line_to_width(Line::from(spans), width)
    }
}

pub(crate) fn render_display_rows(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    rows: &[DisplayRow],
) {
    for (index, row) in rows.iter().enumerate().take(area.height as usize) {
        let row_area = Rect {
            x: area.x,
            y: area.y.saturating_add(index as u16),
            width: area.width,
            height: 1,
        };
        row.line(area.width).render(row_area, buf);
    }
}

pub(crate) fn truncate_line_to_width(mut line: Line<'static>, width: u16) -> Line<'static> {
    let max_width = usize::from(width);
    if line.width() <= max_width {
        return line;
    }
    let mut remaining = max_width;
    let mut spans = Vec::new();
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let content = span.content.into_owned();
        let content_width = UnicodeWidthStr::width(content.as_str());
        if content_width <= remaining {
            remaining = remaining.saturating_sub(content_width);
            spans.push(Span::styled(content, span.style));
            continue;
        }
        let truncated = truncate_display_width(&content, remaining);
        if !truncated.is_empty() {
            spans.push(Span::styled(truncated, span.style));
        }
        break;
    }
    line.spans = spans;
    line
}
