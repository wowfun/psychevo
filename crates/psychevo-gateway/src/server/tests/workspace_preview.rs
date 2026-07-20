#[tokio::test]
async fn workspace_preview_open_returns_text_metadata_and_an_opaque_media_lease() {
    let (_temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("report.pdf"), b"%PDF-1.7\nfixture\n")
        .expect("pdf fixture");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let first = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-open-1")),
            method: "workspace/file/preview/open".to_string(),
            params: Some(json!({ "scope": scope.clone(), "path": "report.pdf" })),
        },
    )
    .await
    .expect("first preview lease");
    let second = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-open-2")),
            method: "workspace/file/preview/open".to_string(),
            params: Some(json!({ "scope": scope, "path": "report.pdf" })),
        },
    )
    .await
    .expect("second preview lease");

    assert_eq!(first["path"], "report.pdf");
    assert_eq!(first["content"], "%PDF-1.7\nfixture\n");
    assert_eq!(first["mediaType"], "application/pdf");
    assert_eq!(
        first["resourcePath"],
        format!(
            "/_gateway/workspace-preview/{}",
            first["resourceId"].as_str().expect("resource id")
        )
    );
    assert!(first["expiresAtMs"].as_i64().is_some());
    let first_id = first["resourceId"].as_str().expect("first id");
    let second_id = second["resourceId"].as_str().expect("second id");
    assert_ne!(first_id, second_id);
    assert_eq!(first_id.len(), 64, "256-bit lease ids are lowercase hex");
    assert!(first_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[tokio::test]
async fn workspace_preview_http_get_serves_one_closed_byte_range_with_security_headers() {
    let (_temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("clip.mp4"), b"0123456789").expect("media fixture");
    let resource_id = open_workspace_preview(&state, "clip.mp4").await;
    let mut headers = HeaderMap::new();
    headers.insert("range", HeaderValue::from_static("bytes=2-5"));

    let response = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        headers,
        AxumPath(resource_id),
    )
    .await;

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-type"], "video/mp4");
    assert_eq!(response.headers()["content-length"], "4");
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(response.headers()["accept-ranges"], "bytes");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["content-security-policy"], "sandbox");
    assert!(response.headers()["etag"].to_str().is_ok());
    let body = to_bytes(response.into_body(), 64)
        .await
        .expect("range body");
    assert_eq!(&body[..], b"2345");
}

#[tokio::test]
async fn workspace_preview_cors_allows_only_configured_workbench_origins() {
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PSYCHEVO_WORKBENCH_ORIGINS".to_string(),
        "https://app.example, https://preview.example".to_string(),
    )]));
    std::fs::write(state.inner.cwd.join("table.csv"), b"name,value\nAda,1\n").expect("csv fixture");
    let resource_id = open_workspace_preview(&state, "table.csv").await;

    let mut allowed_headers = HeaderMap::new();
    allowed_headers.insert("origin", HeaderValue::from_static("https://app.example"));
    let allowed = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::OPTIONS,
        allowed_headers,
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        allowed.headers()["access-control-allow-origin"],
        "https://app.example"
    );
    assert_eq!(allowed.headers()["vary"], "Origin");
    assert_eq!(
        allowed.headers()["access-control-allow-methods"],
        "GET, HEAD, OPTIONS"
    );
    assert_eq!(allowed.headers()["access-control-allow-headers"], "Range");

    let mut allowed_get_headers = HeaderMap::new();
    allowed_get_headers.insert("origin", HeaderValue::from_static("https://app.example"));
    let allowed_get = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        allowed_get_headers,
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(allowed_get.status(), StatusCode::OK);
    assert_eq!(
        allowed_get.headers()["access-control-allow-origin"],
        "https://app.example"
    );
    assert!(
        allowed_get.headers()["access-control-expose-headers"]
            .to_str()
            .expect("exposed headers")
            .contains("Content-Range")
    );

    let mut blocked_headers = HeaderMap::new();
    blocked_headers.insert("origin", HeaderValue::from_static("https://evil.example"));
    let blocked = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        blocked_headers,
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    assert!(
        blocked
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    assert_eq!(blocked.headers()["cache-control"], "no-store");
}

#[tokio::test]
async fn workspace_preview_cors_allows_product_owned_desktop_origins_without_env() {
    let (_temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("manual.pdf"), b"%PDF-1.7\n").expect("pdf fixture");
    let resource_id = open_workspace_preview(&state, "manual.pdf").await;

    for origin in [
        "http://127.0.0.1:5175",
        "http://tauri.localhost",
        "tauri://localhost",
    ] {
        for (method, expected_status) in [
            (axum::http::Method::OPTIONS, StatusCode::NO_CONTENT),
            (axum::http::Method::GET, StatusCode::OK),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "origin",
                HeaderValue::from_str(origin).expect("Desktop origin header"),
            );
            let response = workspace_preview_resource(
                State(state.clone()),
                method,
                headers,
                AxumPath(resource_id.clone()),
            )
            .await;

            assert_eq!(response.status(), expected_status, "origin {origin}");
            assert_eq!(
                response.headers()["access-control-allow-origin"],
                origin,
                "origin {origin}"
            );
        }
    }

    let mut blocked_headers = HeaderMap::new();
    blocked_headers.insert("origin", HeaderValue::from_static("tauri://evil.example"));
    let blocked = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        blocked_headers,
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    assert!(
        blocked
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}

#[tokio::test]
async fn workspace_preview_http_supports_full_head_open_suffix_and_unsatisfied_ranges() {
    let (_temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("sound.mp3"), b"0123456789").expect("audio fixture");
    let resource_id = open_workspace_preview(&state, "sound.mp3").await;

    let full = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(full.headers()["content-length"], "10");
    assert_eq!(
        &to_bytes(full.into_body(), 64).await.expect("full body")[..],
        b"0123456789"
    );

    let mut open_headers = HeaderMap::new();
    open_headers.insert("range", HeaderValue::from_static("bytes=4-"));
    let head = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::HEAD,
        open_headers,
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(head.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(head.headers()["content-range"], "bytes 4-9/10");
    assert_eq!(head.headers()["content-length"], "6");
    assert!(
        to_bytes(head.into_body(), 64)
            .await
            .expect("head body")
            .is_empty()
    );

    let mut suffix_headers = HeaderMap::new();
    suffix_headers.insert("range", HeaderValue::from_static("bytes=-3"));
    let suffix = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        suffix_headers,
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(suffix.headers()["content-range"], "bytes 7-9/10");
    assert_eq!(
        &to_bytes(suffix.into_body(), 64).await.expect("suffix body")[..],
        b"789"
    );

    for invalid in ["bytes=20-", "bytes=0-1,4-5", "items=0-1"] {
        let mut headers = HeaderMap::new();
        headers.insert(
            "range",
            HeaderValue::from_str(invalid).expect("range header"),
        );
        let response = workspace_preview_resource(
            State(state.clone()),
            axum::http::Method::GET,
            headers,
            AxumPath(resource_id.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()["content-range"], "bytes */10");
        assert_eq!(response.headers()["content-type"], "audio/mpeg");
        assert_eq!(response.headers()["content-length"], "0");
        assert!(response.headers()["etag"].to_str().is_ok());
    }
}

#[tokio::test]
async fn workspace_preview_release_and_file_changes_invalidate_the_capability() {
    let (_temp, state) = web_state();
    let path = state.inner.cwd.join("image.png");
    std::fs::write(&path, b"first-png").expect("image fixture");
    let changed_resource_id = open_workspace_preview(&state, "image.png").await;
    std::fs::write(&path, b"other-png").expect("changed image");
    let changed = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(changed_resource_id),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::CONFLICT);

    std::fs::write(&path, b"fresh-png").expect("fresh image");
    let released_resource_id = open_workspace_preview(&state, "image.png").await;
    let (tx, _rx) = mpsc::unbounded_channel();
    let released = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-release")),
            method: "workspace/file/preview/release".to_string(),
            params: Some(json!({ "resourceId": released_resource_id.clone() })),
        },
    )
    .await
    .expect("release preview");
    assert_eq!(released["released"], true);
    let gone = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(released_resource_id),
    )
    .await;
    assert_eq!(gone.status(), StatusCode::GONE);

    for unknown in ["not-a-resource", &"0".repeat(64)] {
        let missing = workspace_preview_resource(
            State(state.clone()),
            axum::http::Method::GET,
            HeaderMap::new(),
            AxumPath(unknown.to_string()),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn workspace_preview_rejects_large_same_size_tail_changes_with_restored_mtime() {
    use std::io::{Seek, SeekFrom, Write};

    let (_temp, state) = web_state();
    let path = state.inner.cwd.join("large.pdf");
    let fixture = vec![b'a'; 1024 * 1024 + 64];
    std::fs::write(&path, &fixture).expect("large preview fixture");
    let original_modified = std::fs::metadata(&path)
        .expect("original metadata")
        .modified()
        .expect("original modification time");
    let original_revision =
        workspace::workspace_file_snapshot_revision(&path).expect("bounded text revision");
    let resource_id = open_workspace_preview(&state, "large.pdf").await;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open fixture for tail rewrite");
    file.seek(SeekFrom::End(-1)).expect("seek to fixture tail");
    file.write_all(b"z").expect("rewrite fixture tail");
    file.sync_all().expect("flush fixture rewrite");
    file.set_times(std::fs::FileTimes::new().set_modified(original_modified))
        .expect("restore modification time");
    drop(file);

    assert_eq!(
        std::fs::metadata(&path)
            .expect("changed metadata")
            .modified()
            .expect("changed modification time"),
        original_modified,
        "the test must restore the lease's modification time"
    );
    assert_eq!(
        std::fs::metadata(&path).expect("changed metadata").len(),
        fixture.len() as u64,
        "the test must preserve the lease's file size"
    );
    assert_eq!(
        workspace::workspace_file_snapshot_revision(&path).expect("changed bounded revision"),
        original_revision,
        "the tail rewrite must bypass the existing bounded text revision"
    );

    let response = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(
        to_bytes(response.into_body(), 64)
            .await
            .expect("conflict body")
            .is_empty(),
        "the changed file must not leak response bytes"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_preview_rejects_a_symlink_swap_after_open() {
    let (temp, state) = web_state();
    let path = state.inner.cwd.join("slides.pptx");
    std::fs::write(&path, b"inside").expect("inside fixture");
    let resource_id = open_workspace_preview(&state, "slides.pptx").await;
    let outside = temp.path().join("outside.pptx");
    std::fs::write(&outside, b"outside").expect("outside fixture");
    std::fs::remove_file(&path).expect("remove inside");
    std::os::unix::fs::symlink(outside, path).expect("swap symlink");

    let response = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[cfg(unix)]
#[test]
fn workspace_preview_read_projection_stays_on_opened_handle_after_path_swap() {
    let temp = tempfile::tempdir().expect("preview fixture root");
    let path = temp.path().join("selected.txt");
    let original = temp.path().join("original.txt");
    let outside = temp.path().join("outside.txt");
    std::fs::write(&path, b"inside snapshot\n").expect("inside fixture");
    std::fs::write(&outside, b"outside secret\n").expect("outside fixture");
    let mut file = std::fs::File::open(&path).expect("open original handle");

    std::fs::rename(&path, &original).expect("move original path");
    std::os::unix::fs::symlink(&outside, &path).expect("replace selected path");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read replacement path"),
        "outside secret\n"
    );

    let read =
        workspace::workspace_file_read_result_from_file(&mut file, "selected.txt".to_string());
    assert_eq!(read.path, "selected.txt");
    assert_eq!(read.content.as_deref(), Some("inside snapshot\n"));
    assert_eq!(read.size_bytes, "inside snapshot\n".len());
    assert_eq!(
        read.revision,
        workspace::workspace_file_snapshot_revision(&original).expect("original revision")
    );
    assert!(read.editable);
    assert!(!read.binary);
    assert!(read.unreadable.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_preview_rejects_same_metadata_symlink_swap_between_lookup_and_file_open() {
    let (temp, state) = web_state();
    let path = state.inner.cwd.join("same-metadata.pdf");
    let outside = temp.path().join("outside-same-metadata.pdf");
    let mut inside_bytes = vec![b'a'; 1024 * 1024 + 2];
    let mut outside_bytes = inside_bytes.clone();
    inside_bytes[1024 * 1024 + 1] = b'i';
    outside_bytes[1024 * 1024 + 1] = b'o';
    std::fs::write(&path, inside_bytes).expect("inside fixture");
    std::fs::write(&outside, outside_bytes).expect("outside fixture");
    let fixed_modified = SystemTime::now()
        .checked_sub(Duration::from_secs(60))
        .expect("fixed modification time");
    for fixture in [&path, &outside] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(fixture)
            .expect("open fixture for timestamp")
            .set_times(std::fs::FileTimes::new().set_modified(fixed_modified))
            .expect("set matching modification time");
    }
    assert_eq!(
        std::fs::metadata(&path)
            .expect("inside metadata")
            .modified()
            .expect("inside modified"),
        std::fs::metadata(&outside)
            .expect("outside metadata")
            .modified()
            .expect("outside modified")
    );
    assert_eq!(
        workspace::workspace_file_snapshot_revision(&path).expect("inside revision"),
        workspace::workspace_file_snapshot_revision(&outside).expect("outside revision"),
        "the replacement must bypass the existing bounded revision check"
    );

    let resource_id = open_workspace_preview(&state, "same-metadata.pdf").await;
    state
        .inner
        .workspace_preview
        .set_before_open_for_tests(move || {
            std::fs::remove_file(&path).expect("remove inside after lookup");
            std::os::unix::fs::symlink(&outside, &path).expect("swap symlink after lookup");
        });

    let response = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(
        to_bytes(response.into_body(), 64)
            .await
            .expect("conflict body")
            .is_empty(),
        "the replacement must not leak response bytes"
    );
}

#[tokio::test]
async fn workspace_preview_successful_access_refreshes_idle_but_not_absolute_expiry() {
    let (_temp, state) = web_state();
    let now = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(10_000));
    state
        .inner
        .workspace_preview
        .set_clock_for_tests(now.clone());
    std::fs::write(state.inner.cwd.join("document.pdf"), b"pdf").expect("pdf fixture");
    let resource_id = open_workspace_preview(&state, "document.pdf").await;

    now.store(1_700_000, std::sync::atomic::Ordering::SeqCst);
    let refreshed = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::HEAD,
        HeaderMap::new(),
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(refreshed.status(), StatusCode::OK);

    now.store(1_820_000, std::sync::atomic::Ordering::SeqCst);
    let past_original_idle = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::HEAD,
        HeaderMap::new(),
        AxumPath(resource_id.clone()),
    )
    .await;
    assert_eq!(past_original_idle.status(), StatusCode::OK);
    now.store(3_620_000, std::sync::atomic::Ordering::SeqCst);
    let idle_expired = workspace_preview_resource(
        State(state),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(resource_id),
    )
    .await;
    assert_eq!(idle_expired.status(), StatusCode::GONE);

    let (_temp, absolute_state) = web_state();
    let absolute_now = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    absolute_state
        .inner
        .workspace_preview
        .set_clock_for_tests(absolute_now.clone());
    std::fs::write(absolute_state.inner.cwd.join("movie.webm"), b"webm").expect("webm fixture");
    let absolute_id = open_workspace_preview(&absolute_state, "movie.webm").await;
    for access_at_ms in (1..=16).map(|step| step * 1_700_000) {
        absolute_now.store(access_at_ms, std::sync::atomic::Ordering::SeqCst);
        let keep_alive = workspace_preview_resource(
            State(absolute_state.clone()),
            axum::http::Method::HEAD,
            HeaderMap::new(),
            AxumPath(absolute_id.clone()),
        )
        .await;
        assert_eq!(keep_alive.status(), StatusCode::OK);
    }
    absolute_now.store(28_700_000, std::sync::atomic::Ordering::SeqCst);
    let near_absolute = workspace_preview_resource(
        State(absolute_state.clone()),
        axum::http::Method::HEAD,
        HeaderMap::new(),
        AxumPath(absolute_id.clone()),
    )
    .await;
    assert_eq!(near_absolute.status(), StatusCode::OK);
    absolute_now.store(28_800_000, std::sync::atomic::Ordering::SeqCst);
    let absolute_expired = workspace_preview_resource(
        State(absolute_state),
        axum::http::Method::GET,
        HeaderMap::new(),
        AxumPath(absolute_id),
    )
    .await;
    assert_eq!(absolute_expired.status(), StatusCode::GONE);
}

#[tokio::test]
async fn workspace_preview_open_rejects_traversal_and_a_browser_scope_pivot() {
    let (temp, state) = web_state();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside workspace");
    std::fs::write(outside.join("secret.pdf"), b"secret").expect("outside fixture");
    let current_scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let outside_scope = wire::GatewayRequestScope {
        cwd: outside.to_string_lossy().to_string(),
        source: wire::GatewaySourceInput {
            kind: "web".to_string(),
            raw_id: Some("outside-preview".to_string()),
            lifetime: None,
            raw_identity: None,
            visible_name: None,
        },
    };
    let session_id = "browser-preview-scope".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let (tx, _rx) = mpsc::unbounded_channel();

    let traversal = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-traversal")),
            method: "workspace/file/preview/open".to_string(),
            params: Some(json!({
                "scope": current_scope,
                "path": "../outside/secret.pdf"
            })),
        },
    )
    .await
    .expect_err("traversal must be rejected");
    assert_eq!(traversal.to_string(), "workspace path must be relative");

    let pivot = handle_rpc(
        state,
        AuthContext::Browser { session_id },
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-scope-pivot")),
            method: "workspace/file/preview/open".to_string(),
            params: Some(json!({ "scope": outside_scope, "path": "secret.pdf" })),
        },
    )
    .await
    .expect_err("browser scope pivot must be rejected");
    assert!(pivot.to_string().contains("not authorized"));
}

#[tokio::test]
async fn workspace_preview_options_and_invalid_ranges_do_not_refresh_idle_expiry() {
    let (_temp, state) = web_state();
    let now = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    state
        .inner
        .workspace_preview
        .set_clock_for_tests(now.clone());
    std::fs::write(state.inner.cwd.join("fixture.pdf"), b"fixture").expect("pdf fixture");
    let options_id = open_workspace_preview(&state, "fixture.pdf").await;
    let range_id = open_workspace_preview(&state, "fixture.pdf").await;

    now.store(1_700_000, std::sync::atomic::Ordering::SeqCst);
    let options = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::OPTIONS,
        HeaderMap::new(),
        AxumPath(options_id.clone()),
    )
    .await;
    assert_eq!(options.status(), StatusCode::NO_CONTENT);
    let mut invalid_range = HeaderMap::new();
    invalid_range.insert("range", HeaderValue::from_static("bytes=99-"));
    let invalid = workspace_preview_resource(
        State(state.clone()),
        axum::http::Method::GET,
        invalid_range,
        AxumPath(range_id.clone()),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::RANGE_NOT_SATISFIABLE);

    now.store(1_800_000, std::sync::atomic::Ordering::SeqCst);
    for resource_id in [options_id, range_id] {
        let expired = workspace_preview_resource(
            State(state.clone()),
            axum::http::Method::GET,
            HeaderMap::new(),
            AxumPath(resource_id),
        )
        .await;
        assert_eq!(expired.status(), StatusCode::GONE);
    }
}

async fn open_workspace_preview(state: &WebState, path: &str) -> String {
    let scope = default_resolved_scope(state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("preview-open")),
            method: "workspace/file/preview/open".to_string(),
            params: Some(json!({ "scope": scope, "path": path })),
        },
    )
    .await
    .expect("preview lease")["resourceId"]
        .as_str()
        .expect("resource id")
        .to_string()
}
