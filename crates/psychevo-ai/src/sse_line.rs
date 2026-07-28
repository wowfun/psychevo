pub(crate) fn next_sse_line(buffer: &[u8], finish: bool) -> Option<(usize, usize)> {
    let pos = buffer
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r');
    match pos {
        Some(index) => {
            if buffer[index] == b'\r' && buffer.get(index + 1).is_none() && !finish {
                return None;
            }
            let consumed =
                if buffer[index] == b'\r' && buffer.get(index + 1).copied() == Some(b'\n') {
                    index + 2
                } else {
                    index + 1
                };
            Some((index, consumed))
        }
        None if finish && !buffer.is_empty() => Some((buffer.len(), buffer.len())),
        None => None,
    }
}
