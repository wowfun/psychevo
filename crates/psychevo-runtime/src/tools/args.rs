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
