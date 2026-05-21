fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Message(format!("{key} must be a string")))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>> {
    args.get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::Message(format!("{key} must be a string")))
        })
        .transpose()
}

fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>> {
    args.get(key)
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>> {
    args.get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| Error::Message(format!("{key} must be a boolean")))
        })
        .transpose()
}

fn bounded_limit(value: Option<i64>, default: usize, max: usize) -> Result<usize> {
    let limit = value.unwrap_or(default as i64);
    if limit < 1 {
        return Err(Error::Message("limit must be >= 1".to_string()));
    }
    Ok((limit as usize).min(max))
}

fn truncate_match_line(line: &str) -> String {
    const MAX_LINE_CHARS: usize = 240;
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut value = line.chars().take(MAX_LINE_CHARS).collect::<String>();
    value.push_str("...");
    value
}
