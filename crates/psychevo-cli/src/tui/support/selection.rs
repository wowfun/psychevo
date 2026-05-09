impl ScreenLine {
    #[cfg(test)]
    fn first_x(&self) -> u16 {
        self.cells.first().map(|cell| cell.x).unwrap_or(0)
    }

    #[cfg(test)]
    fn text(&self) -> String {
        self.cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
    }
}

#[cfg(test)]
fn screen_cells_from_text(start_x: u16, text: &str) -> Vec<ScreenCell> {
    let mut cells: Vec<ScreenCell> = Vec::new();
    let mut x = start_x;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0) as u16;
        if width == 0 {
            if let Some(cell) = cells.last_mut() {
                cell.text.push(ch);
            }
            continue;
        }
        cells.push(ScreenCell {
            x,
            width,
            text: ch.to_string(),
        });
        x = x.saturating_add(width);
    }
    cells
}

fn screen_line_from_buffer(
    buffer: &ratatui::buffer::Buffer,
    start_x: u16,
    y: u16,
    width: u16,
    region: SelectableRegion,
) -> Option<ScreenLine> {
    let mut cells = Vec::new();
    let right = start_x.saturating_add(width);
    let mut x = start_x;
    while x < right {
        let Some(cell) = buffer.cell((x, y)) else {
            break;
        };
        let symbol = cell.symbol();
        let symbol_width = UnicodeWidthStr::width(symbol).max(1) as u16;
        let width = symbol_width.min(right.saturating_sub(x)).max(1);
        cells.push(ScreenCell {
            x,
            width,
            text: symbol.to_string(),
        });
        x = x.saturating_add(width);
    }
    trim_screen_padding_right(&mut cells);
    (!cells.is_empty()).then_some(ScreenLine { region, y, cells })
}

fn trim_screen_padding_right(cells: &mut Vec<ScreenCell>) {
    while cells.last().is_some_and(|cell| cell.text == " ") {
        cells.pop();
    }
}

fn selected_text_from_lines(lines: &[ScreenLine], selection: &SelectionState) -> Option<String> {
    let anchor = selection.anchor?;
    let focus = selection.focus?;
    if anchor == focus {
        return None;
    }
    let ((start_x, start_y), (end_x, end_y)) = ordered_selection(anchor, focus);
    let mut pieces = Vec::new();
    for y in start_y..=end_y {
        let from = if y == start_y { start_x } else { 0 };
        let to = if y == end_y { end_x } else { u16::MAX };
        let mut segments = lines
            .iter()
            .filter(|line| selection.region.is_none_or(|region| line.region == region))
            .filter(|line| line.y == y)
            .filter_map(|line| selected_segment_from_line(line, from, to))
            .collect::<Vec<_>>();
        segments.sort_by_key(|segment| segment.start_x);
        let mut row_text = String::new();
        let mut cursor = None;
        for segment in segments {
            if let Some(cursor_x) = cursor
                && segment.start_x > cursor_x
            {
                row_text.push_str(&" ".repeat(usize::from(segment.start_x - cursor_x)));
            }
            row_text.push_str(&segment.text);
            cursor = Some(segment.end_x);
        }
        if !row_text.is_empty() {
            pieces.push(row_text);
        }
    }
    (!pieces.is_empty()).then(|| pieces.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedSegment {
    start_x: u16,
    end_x: u16,
    text: String,
}

fn selected_segment_from_line(line: &ScreenLine, from: u16, to: u16) -> Option<SelectedSegment> {
    if to <= from {
        return None;
    }
    let mut text = String::new();
    let mut start_x = None;
    let mut end_x = None;
    let mut cursor = None;
    for cell in &line.cells {
        if !cell_overlaps_range(cell, from, to) {
            continue;
        }
        if start_x.is_none() {
            start_x = Some(cell.x);
        }
        if let Some(cursor_x) = cursor
            && cell.x > cursor_x
        {
            text.push_str(&" ".repeat(usize::from(cell.x - cursor_x)));
        }
        text.push_str(&cell.text);
        let cell_end = cell.x.saturating_add(cell.width);
        end_x = Some(cell_end);
        cursor = Some(cell_end);
    }
    Some(SelectedSegment {
        start_x: start_x?,
        end_x: end_x?,
        text,
    })
}

fn cell_overlaps_range(cell: &ScreenCell, from: u16, to: u16) -> bool {
    let cell_end = cell.x.saturating_add(cell.width);
    to > cell.x && from < cell_end
}

fn ordered_selection(anchor: (u16, u16), focus: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    let (ax, ay) = anchor;
    let (fx, fy) = focus;
    if (ay, ax) <= (fy, fx) {
        ((ax, ay), (fx, fy))
    } else {
        ((fx, fy), (ax, ay))
    }
}

