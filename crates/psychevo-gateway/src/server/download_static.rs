async fn download_session(
    State(state): State<WebState>,
    headers: HeaderMap,
    AxumPath((session_id, kind)): AxumPath<(String, String)>,
    Query(query): Query<DownloadQuery>,
) -> impl IntoResponse {
    let Some(auth) = state.auth_from_headers(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Err(err) = authorize_thread(&state, &auth, &session_id) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": {"message": err.to_string()}})),
        )
            .into_response();
    }
    match render_download(&state, &session_id, &kind, &query) {
        Ok(response) => response.into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": err.to_string()}})),
        )
            .into_response(),
    }
}

async fn read_media_artifact(
    State(state): State<WebState>,
    headers: HeaderMap,
    AxumPath(artifact_id): AxumPath<String>,
) -> impl IntoResponse {
    if state.auth_from_headers(&headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match render_media_artifact(&state, &artifact_id) {
        Ok(response) => response.into_response(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": err.to_string()}})),
        )
            .into_response(),
    }
}

fn render_media_artifact(
    state: &WebState,
    artifact_id: &str,
) -> psychevo_runtime::Result<Response<Body>> {
    psychevo_runtime::validate_media_artifact_id(artifact_id)?;
    let path = psychevo_runtime::media_artifact_path(&state.inner.home, artifact_id)?;
    let bytes = std::fs::read(&path)?;
    let media = psychevo_runtime::read_media_artifact(&state.inner.home, artifact_id)?;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&media.mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "inline; filename=\"{}.{}\"",
            artifact_id,
            media
                .mime_type
                .split('/')
                .nth(1)
                .unwrap_or("img")
                .replace("jpeg", "jpg")
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("inline")),
    );
    Ok(response)
}

fn render_download(
    state: &WebState,
    session_id: &str,
    kind: &str,
    query: &DownloadQuery,
) -> psychevo_runtime::Result<Response<Body>> {
    let artifact_kind = match kind {
        "export" => SessionArtifactKind::Export,
        "share" => SessionArtifactKind::Share,
        value => return Err(Error::Message(format!("unknown download kind: {value}"))),
    };
    let format = match query
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => parse_session_export_format(value)
            .ok_or_else(|| Error::Message(format!("unknown export format: {value}")))?,
        None => SessionExportFormat::Markdown,
    };
    if artifact_kind == SessionArtifactKind::Share && format != SessionExportFormat::Markdown {
        return Err(Error::Message(
            "share artifacts support only markdown format".to_string(),
        ));
    }
    let include = match query
        .include
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => SessionExportIncludeSet::parse(value, artifact_kind)?,
        None => SessionExportIncludeSet::default_for(artifact_kind),
    };
    let artifact = render_session_export(
        state.inner.state.store(),
        session_id,
        SessionExportOptions {
            format,
            include,
            artifact_kind,
        },
    )?;
    let filename = query
        .filename
        .as_deref()
        .and_then(|filename| sanitize_download_filename(filename, artifact.format))
        .unwrap_or_else(|| format!("{kind}-{session_id}.{}", artifact.format.extension()));
    let mut response = Response::new(Body::from(artifact.content));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(content_type_for_export_format(artifact.format)),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

#[derive(Debug, Default, Deserialize)]
struct DownloadQuery {
    format: Option<String>,
    include: Option<String>,
    filename: Option<String>,
}

fn content_type_for_export_format(format: SessionExportFormat) -> &'static str {
    match format {
        SessionExportFormat::Markdown => "text/markdown; charset=utf-8",
        SessionExportFormat::Json => "application/json; charset=utf-8",
    }
}

fn sanitize_download_filename(value: &str, format: SessionExportFormat) -> Option<String> {
    let basename = value.rsplit(['/', '\\']).next().unwrap_or(value).trim();
    if basename.is_empty() || basename == "." || basename == ".." {
        return None;
    }
    let sanitized = basename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .chars()
        .take(180)
        .collect::<String>();
    if sanitized.is_empty() {
        return None;
    }
    Some(download_filename_with_format_extension(&sanitized, format))
}

fn download_filename_with_format_extension(filename: &str, format: SessionExportFormat) -> String {
    let extension = format.extension();
    let lower = filename.to_ascii_lowercase();
    let stem = if let Some(stripped) = lower
        .ends_with(".json")
        .then(|| filename.strip_suffix(&filename[filename.len() - 5..]))
        .flatten()
    {
        stripped
    } else if let Some(stripped) = lower
        .ends_with(".markdown")
        .then(|| filename.strip_suffix(&filename[filename.len() - 9..]))
        .flatten()
    {
        stripped
    } else if let Some(stripped) = lower
        .ends_with(".md")
        .then(|| filename.strip_suffix(&filename[filename.len() - 3..]))
        .flatten()
    {
        stripped
    } else {
        filename
    };
    format!("{stem}.{extension}")
}

async fn static_asset(
    State(state): State<WebState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> impl IntoResponse {
    let Some(static_dir) = &state.inner.static_dir else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let request_path = uri.path().trim_start_matches('/');
    let candidate = if request_path.is_empty() {
        static_dir.join("index.html")
    } else {
        static_dir.join(request_path)
    };
    let candidate_is_file = tokio::fs::metadata(&candidate)
        .await
        .is_ok_and(|metadata| metadata.is_file());
    let serves_shell = request_path.is_empty() || request_path == "index.html" || !candidate_is_file;
    if serves_shell && state.auth_from_headers(&headers).is_none() {
        return launch_required_page().into_response();
    }
    let path = if candidate_is_file {
        candidate
    } else {
        static_dir.join("index.html")
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static(content_type_for_path(&path)),
            );
            response.headers_mut().insert(
                CACHE_CONTROL,
                HeaderValue::from_static(if candidate_is_file && is_fingerprinted_asset(request_path) {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-store"
                }),
            );
            response.into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            "Workbench assets not found. Run `pnpm --filter @psychevo/workbench build` or pass --static-dir.",
        )
            .into_response(),
    }
}

fn is_fingerprinted_asset(path: &str) -> bool {
    let Some(filename) = path.strip_prefix("assets/") else {
        return false;
    };
    let stem = filename
        .strip_suffix(".map")
        .unwrap_or(filename)
        .rsplit_once('.')
        .map_or(filename, |(stem, _)| stem);
    let bytes = stem.as_bytes();
    if bytes.len() < 9 || bytes[bytes.len() - 9] != b'-' {
        return false;
    }
    bytes[bytes.len() - 8..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn launch_required_page() -> Response<Body> {
    let body = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>pevo launch required</title>
    <style>
      :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }
      main { max-width: 560px; padding: 32px; line-height: 1.5; }
      h1 { margin: 0 0 12px; font-size: 24px; }
      p { margin: 0 0 14px; }
      code { padding: 2px 6px; border: 1px solid color-mix(in srgb, CanvasText 18%, transparent); border-radius: 6px; }
    </style>
  </head>
  <body>
    <main>
      <h1>pevo launch required</h1>
      <p>This local Workbench URL needs a browser-session cookie created by the launch flow.</p>
      <p>Run <code>pevo web</code>, or run <code>pevo web --print-url</code> and open the returned <code>openUrl</code>.</p>
      <p>Do not open the managed <code>baseUrl</code> directly.</p>
    </main>
  </body>
</html>"#;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn launch_expired_page(status: StatusCode) -> Response<Body> {
    let body = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>pevo launch link expired</title>
    <style>
      :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }
      main { max-width: 560px; padding: 32px; line-height: 1.5; }
      h1 { margin: 0 0 12px; font-size: 24px; }
      p { margin: 0 0 14px; }
      code { padding: 2px 6px; border: 1px solid color-mix(in srgb, CanvasText 18%, transparent); border-radius: 6px; }
    </style>
  </head>
  <body>
    <main>
      <h1>pevo launch link expired</h1>
      <p>This <code>openUrl</code> was already used, expired, or opened in a browser without the launch cookie.</p>
      <p>Run <code>pevo web</code>, or run <code>pevo web --print-url</code> and open the new <code>openUrl</code>.</p>
      <p>If the Workbench already launched in this browser, open the clean local URL shown as <code>baseUrl</code>.</p>
    </main>
  </body>
</html>"#;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
async fn gateway_fallback(
    State(state): State<WebState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> impl IntoResponse {
    if uri.path().starts_with("/_gateway/managed/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    static_asset(State(state), headers, uri).await.into_response()
}
