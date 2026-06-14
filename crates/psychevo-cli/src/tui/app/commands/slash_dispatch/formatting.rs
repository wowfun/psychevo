pub(crate) fn format_exec_prefix_for_status(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(|value| match value {
            Value::String(raw) => Some(raw.clone()),
            Value::Array(alternatives) => Some(format!(
                "[{}]",
                alternatives
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|")
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
