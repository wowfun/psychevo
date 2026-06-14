pub fn read_session_trace(
    db_path: &Path,
    session_id: &str,
    options: SessionTraceReadOptions,
) -> SessionTraceReadResult {
    let mut warnings = Vec::new();
    let Some(path) = (match session_trace_path(db_path, session_id) {
        Ok(path) => path,
        Err(err) => {
            warnings.push(err);
            None
        }
    }) else {
        return SessionTraceReadResult {
            thread_id: session_id.to_string(),
            available: false,
            events: Vec::new(),
            warnings,
            truncated: false,
            next_after_seq: options.after_seq,
        };
    };
    if !path.exists() {
        return SessionTraceReadResult {
            thread_id: session_id.to_string(),
            available: false,
            events: Vec::new(),
            warnings,
            truncated: false,
            next_after_seq: options.after_seq,
        };
    }
    let limit = options
        .limit
        .unwrap_or(SESSION_TRACE_DEFAULT_LIMIT)
        .clamp(1, SESSION_TRACE_MAX_LIMIT);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) => {
            warnings.push(format!("failed to read session trace: {err}"));
            return SessionTraceReadResult {
                thread_id: session_id.to_string(),
                available: true,
                events: Vec::new(),
                warnings,
                truncated: false,
                next_after_seq: options.after_seq,
            };
        }
    };
    let mut events = VecDeque::new();
    let mut truncated = false;
    let mut pending_malformed: Option<(usize, String)> = None;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                warnings.push(format!("failed to read session trace: {err}"));
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((line_no, message)) = pending_malformed.take() {
            warnings.push(format!(
                "ignored malformed session trace line {line_no}: {message}",
            ));
        }
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => {
                warnings.push(format!(
                    "ignored non-object session trace line {}",
                    index + 1
                ));
                continue;
            }
            Err(err) => {
                pending_malformed = Some((index + 1, err.to_string()));
                continue;
            }
        };
        let seq = value.get("seq").and_then(Value::as_u64).unwrap_or(0);
        if options.after_seq.is_none_or(|after_seq| seq > after_seq) {
            if options.after_seq.is_some() && events.len() >= limit {
                truncated = true;
                break;
            }
            events.push_back(value);
            if options.after_seq.is_none() && events.len() > limit {
                let _ = events.pop_front();
                truncated = true;
            }
        }
    }
    if let Some((_line_no, message)) = pending_malformed {
        warnings.push(format!(
            "ignored malformed final session trace line: {message}"
        ));
    }

    let events = events.into_iter().collect::<Vec<_>>();
    let next_after_seq = events
        .last()
        .and_then(|value| value.get("seq"))
        .and_then(Value::as_u64)
        .or(options.after_seq);
    SessionTraceReadResult {
        thread_id: session_id.to_string(),
        available: true,
        events,
        warnings,
        truncated,
        next_after_seq,
    }
}

pub(crate) fn remove_session_trace_dir(db_path: &Path, session_id: &str) -> Result<(), String> {
    let Some(path) = session_trace_path(db_path, session_id)? else {
        return Ok(());
    };
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|err| format!("failed to remove session trace directory: {err}"))?;
    }
    Ok(())
}

pub fn session_trace_path(db_path: &Path, session_id: &str) -> Result<Option<PathBuf>, String> {
    if db_path == Path::new(":memory:") {
        return Ok(None);
    }
    validate_session_trace_id(session_id)?;
    let root = db_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Some(
        root.join("sessions").join(session_id).join("events.jsonl"),
    ))
}

fn append_trace_record(
    path: &Path,
    session_id: &str,
    invocation_id: &str,
    seq: u64,
    draft: SessionTraceDraft,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create session trace directory: {err}"))?;
    }
    let record = json!({
        "schema_version": SESSION_TRACE_SCHEMA_VERSION,
        "seq": seq,
        "event_id": Uuid::now_v7().to_string(),
        "session_id": session_id,
        "invocation_id": invocation_id,
        "turn_index": draft.turn_index,
        "kind": draft.kind,
        "timestamp_ms": draft.timestamp_ms,
        "monotonic_offset_ms": draft.monotonic_offset_ms,
        "source": "runtime",
        "correlation": draft.correlation,
        "redaction_state": "redacted",
        "payload": draft.payload,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("failed to open session trace: {err}"))?;
    serde_json::to_writer(&mut file, &record)
        .map_err(|err| format!("failed to encode session trace event: {err}"))?;
    file.write_all(b"\n")
        .map_err(|err| format!("failed to write session trace event: {err}"))?;
    Ok(())
}

fn max_valid_seq(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let file =
        fs::File::open(path).map_err(|err| format!("failed to read session trace seq: {err}"))?;
    let mut seq = 0;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| format!("failed to read session trace seq: {err}"))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(value) = value.get("seq").and_then(Value::as_u64) {
            seq = seq.max(value);
        }
    }
    Ok(seq)
}

fn set_last_error(slot: &Arc<Mutex<Option<String>>>, message: String) {
    if let Ok(mut current) = slot.lock() {
        *current = Some(message);
    }
}
