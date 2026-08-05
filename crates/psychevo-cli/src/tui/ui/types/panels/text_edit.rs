use crate::tui::UnicodeWidthStr;

pub(super) fn wrapped_height(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let display_width = UnicodeWidthStr::width(text);
    display_width.div_ceil(width).max(1) as u16
}

pub(super) fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn byte_index_for_char(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .map(|(index, _)| index)
        .nth(char_index)
        .unwrap_or(value.len())
}

pub(super) fn insert_char(value: &mut String, cursor: &mut usize, ch: char) {
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    let byte_index = byte_index_for_char(value, *cursor);
    value.insert(byte_index, ch);
    *cursor += 1;
}

pub(super) fn remove_previous_char(value: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    let start = byte_index_for_char(value, (*cursor).saturating_sub(1));
    let end = byte_index_for_char(value, *cursor);
    value.replace_range(start..end, "");
    *cursor = (*cursor).saturating_sub(1);
}

pub(super) fn remove_next_char(value: &mut String, cursor: &mut usize) {
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    if *cursor >= len {
        return;
    }
    let start = byte_index_for_char(value, *cursor);
    let end = byte_index_for_char(value, (*cursor).saturating_add(1));
    value.replace_range(start..end, "");
}

pub(super) fn move_cursor(value: &str, cursor: &mut usize, delta: isize) {
    let len = char_count(value);
    let current = (*cursor).min(len) as isize;
    *cursor = (current + delta).clamp(0, len as isize) as usize;
}
