use crate::*;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

const SERVE_FILE_LIMIT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub config: Option<PathBuf>,
    pub benchmark: Option<String>,
    pub report: Option<String>,
    pub store_root: Option<PathBuf>,
    pub path: Option<PathBuf>,
    pub task_set: Option<String>,
    pub agent: Option<String>,
    pub task: Option<String>,
    pub status: Option<CaseStatusFilter>,
    pub host: IpAddr,
    pub port: u16,
}

impl ServeOptions {
    pub fn view_request(&self, service: &EvalService) -> ServiceResult<ViewRequest> {
        let mut request = ViewRequest {
            config: self.config.clone(),
            benchmark: self.benchmark.clone(),
            report: self.report.clone(),
            store_root: self.store_root.clone(),
            path: self.path.clone(),
            task_set: self.task_set.clone(),
            agent: self.agent.clone(),
            task: self.task.clone(),
            status: self.status,
            group_by: Vec::new(),
            include: all_view_includes(),
        };
        if request.config.is_none() && request.benchmark.is_none() && request.path.is_none() {
            let store = service.store(request.store_root.clone())?;
            request.path = Some(store.root.join("runs"));
        }
        Ok(request)
    }
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            config: None,
            benchmark: None,
            report: None,
            store_root: None,
            path: None,
            task_set: None,
            agent: None,
            task: None,
            status: None,
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ServeState {
    service: EvalService,
    view_request: ViewRequest,
    token: String,
    workspace_root: PathBuf,
}

pub fn run_serve_blocking(service: EvalService, options: ServeOptions) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build peval serve runtime")?;
    runtime.block_on(run_serve(service, options))
}

pub async fn run_serve(service: EvalService, options: ServeOptions) -> Result<()> {
    let view_request = options.view_request(&service).map_err(anyhow::Error::new)?;
    let store = service
        .store(view_request.store_root.clone())
        .map_err(anyhow::Error::new)?;
    let token = Uuid::now_v7().to_string();
    let state = Arc::new(ServeState {
        service,
        view_request,
        token: token.clone(),
        workspace_root: store.root,
    });
    let app = serve_router(state);
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(options.host, options.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", options.host, options.port))?;
    let addr = listener.local_addr()?;
    let display_host = if addr.ip().is_unspecified() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        addr.ip()
    };
    println!(
        "peval serve: http://{display_host}:{}?token={token}",
        addr.port()
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("peval serve failed")
}

pub(crate) fn serve_router(state: Arc<ServeState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/view", get(serve_view_json))
        .route("/file/{*path}", get(serve_file))
        .route("/ws", get(serve_ws))
        .with_state(state)
}

async fn serve_index(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if !token_is_valid(&state, &query) {
        return unauthorized_response();
    }
    Html(serve_index_html(&state.token)).into_response()
}

async fn serve_view_json(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if !token_is_valid(&state, &query) {
        return unauthorized_response();
    }
    match build_service_view(&state) {
        Ok(view) => Json(view).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn serve_file(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<BTreeMap<String, String>>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if !token_is_valid(&state, &query) {
        return unauthorized_response();
    }
    match read_bounded_workspace_file(&state.workspace_root, Path::new(&path)) {
        Ok((bytes, mime)) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    }
}

async fn serve_ws(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<BTreeMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    if !token_is_valid(&state, &query) {
        return unauthorized_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<ServeState>) {
    let (mut sender, mut receiver) = socket.split();
    while let Some(Ok(message)) = receiver.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let response = handle_rpc_message(&state, &text);
        if sender
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

pub(crate) fn handle_rpc_message(state: &ServeState, text: &str) -> Value {
    let request = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "view.get" => match build_service_view(state) {
            Ok(view) => json!({ "id": id, "result": view }),
            Err(err) => json!({ "id": id, "error": service_error(&err) }),
        },
        "analysis.status" => match state.service.analysis_status(&state.view_request) {
            Ok(status) => json!({ "id": id, "result": status }),
            Err(err) => json!({ "id": id, "error": { "code": err.code, "message": err.message } }),
        },
        "analysis.run" => {
            let trial_key = request
                .get("params")
                .and_then(|params| params.get("trial_key"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let overwrite = request
                .get("params")
                .and_then(|params| params.get("overwrite"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match state.service.analyze_trial(AnalysisTrialRequest {
                view: state.view_request.clone(),
                trial_key,
                overwrite,
            }) {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(err) => {
                    json!({ "id": id, "error": { "code": err.code, "message": err.message } })
                }
            }
        }
        "analysis.batch_failed" => {
            let overwrite = request
                .get("params")
                .and_then(|params| params.get("overwrite"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match state.service.analyze_failed_batch(AnalysisBatchRequest {
                view: state.view_request.clone(),
                overwrite,
            }) {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(err) => {
                    json!({ "id": id, "error": { "code": err.code, "message": err.message } })
                }
            }
        }
        _ => json!({
            "id": id,
            "error": {
                "code": "method_not_found",
                "message": format!("unknown method `{method}`")
            }
        }),
    }
}

fn build_service_view(state: &ServeState) -> std::result::Result<ViewReport, Box<EvalDiagnostic>> {
    state
        .service
        .view(state.view_request.clone())
        .map_err(Box::new)
}

fn service_error(err: &EvalDiagnostic) -> Value {
    json!({
        "code": err.code,
        "message": err.message
    })
}

fn token_is_valid(state: &ServeState, query: &BTreeMap<String, String>) -> bool {
    query
        .get("token")
        .is_some_and(|token| token == &state.token)
}

fn unauthorized_response() -> Response {
    (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response()
}

pub(crate) fn read_bounded_workspace_file(
    root: &Path,
    relative: &Path,
) -> Result<(Vec<u8>, String)> {
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("file path must be relative to the peval workspace");
    }
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let candidate = canonical_root.join(relative);
    let canonical_path = fs::canonicalize(&candidate)
        .with_context(|| format!("failed to canonicalize {}", candidate.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!("file path escapes peval workspace");
    }
    let metadata = fs::metadata(&canonical_path)
        .with_context(|| format!("failed to stat {}", canonical_path.display()))?;
    if !metadata.is_file() {
        bail!("file path is not a regular file");
    }
    if metadata.len() > SERVE_FILE_LIMIT_BYTES {
        bail!("file exceeds 1 MiB raw/detail limit");
    }
    let bytes = fs::read(&canonical_path)
        .with_context(|| format!("failed to read {}", canonical_path.display()))?;
    Ok((bytes, mime_for_path(relative).to_string()))
}

fn serve_index_html(token: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>peval serve</title>
{css}
</head>
<body>
<main class="page">
<section class="mast"><div><p class="eyebrow">peval serve</p><h1>Evaluation Workbench</h1><p class="subline" id="summary">Loading workspace trials...</p></div><div class="verdict" id="verdict">...</div></section>
<section class="toolbar"><input id="search" placeholder="Filter task, agent, status"><button id="refresh" type="button">Refresh</button><button id="analyze" type="button" disabled>Analyze Trial</button><button id="batch" type="button" disabled>Analyze Failed</button></section>
<section class="ledger"><h2>Matrix</h2><div id="matrix"></div></section>
<section class="ledger"><h2>Trial</h2><div id="trial"></div></section>
</main>
<script>
const TOKEN = "{token}";
const state = {{ view: null, selectedTrial: null, analysisEnabled: false }};
const $ = (id) => document.getElementById(id);
function rpc(method, params={{}}) {{
  return new Promise((resolve, reject) => {{
    const ws = new WebSocket(`${{location.protocol === "https:" ? "wss" : "ws"}}://${{location.host}}/ws?token=${{encodeURIComponent(TOKEN)}}`);
    ws.onopen = () => ws.send(JSON.stringify({{id: 1, method, params}}));
    ws.onmessage = (event) => {{
      const msg = JSON.parse(event.data);
      ws.close();
      msg.error ? reject(msg.error) : resolve(msg.result);
    }};
    ws.onerror = () => reject(new Error("websocket failed"));
  }});
}}
function esc(value) {{
  return String(value ?? "").replace(/[&<>\"]/g, ch => ({{"&":"&amp;","<":"&lt;",">":"&gt;","\"":"&quot;"}}[ch]));
}}
function statusClass(status) {{
  return status === "Passed" ? "present" : "failed";
}}
function render(view) {{
  state.view = view;
  $("summary").textContent = `${{view.summary.total_trials}} trials - ${{view.summary.passed_trials}} passed - ${{view.summary.failed_trials}} failed`;
  $("verdict").textContent = view.summary.status;
  $("verdict").className = `verdict ${{view.summary.status === "Passed" ? "present" : "failed"}}`;
  renderMatrix();
  selectTrial(state.selectedTrial || view.trials[0]?.trial_key);
}}
function renderMatrix() {{
  const query = $("search").value.toLowerCase();
  const cells = state.view.matrix.cells.filter(cell => JSON.stringify(cell).toLowerCase().includes(query));
  $("matrix").innerHTML = `<table><thead><tr><th>task</th><th>agent</th><th>status</th><th>score</th><th>duration</th><th>tools</th></tr></thead><tbody>${{cells.map(cell => `<tr data-trial="${{esc(cell.representative_trial_key)}}"><td>${{esc(cell.task_id)}}</td><td>${{esc(cell.agent_id)}}</td><td><span class="stamp ${{statusClass(cell.status)}}">${{esc(cell.status)}}</span></td><td class="num">${{esc(cell.score ?? "-")}}</td><td class="num">${{esc(cell.duration_ms)}}</td><td class="num">${{esc(cell.tool_calls)}}/${{esc(cell.tool_errors)}}</td></tr>`).join("")}}</tbody></table>`;
  document.querySelectorAll("#matrix tr[data-trial]").forEach(row => row.addEventListener("click", () => selectTrial(row.dataset.trial)));
}}
function selectTrial(trialKey) {{
  if (!trialKey) return;
  state.selectedTrial = trialKey;
  const trial = state.view.trials.find(row => row.trial_key === trialKey);
  const trajectory = state.view.trajectory.find(row => row.trial_key === trialKey);
  const analysis = state.view.analysis.find(row => row.trial_key === trialKey);
  $("trial").innerHTML = `<h3>${{esc(trialKey)}}</h3>${{trial ? `<p class="subline">${{esc(trial.task_id)}} / ${{esc(trial.agent_id)}} / ${{esc(trial.status)}}</p>` : ""}}${{trajectory ? renderTrajectory(trajectory) : "<p>No trajectory include.</p>"}}${{analysis ? `<h3>Analysis</h3><pre>${{esc(analysis.summary || analysis.status)}}</pre>` : ""}}`;
}}
function renderTrajectory(trajectory) {{
  const maxTokens = Math.max(...trajectory.steps.map(step => step.token_total || 0), 1);
  return `<h3>Trajectory</h3><table><thead><tr><th>step</th><th>source</th><th>label</th><th>tokens</th><th>summary</th></tr></thead><tbody>${{trajectory.steps.map(step => `<tr><td class="num">${{step.step_id}}</td><td>${{esc(step.source)}}</td><td>${{esc(step.label)}}</td><td><div class="bars"><b style="width:${{Math.max(((step.token_total || 0) / maxTokens) * 100, step.token_total ? 4 : 0)}}%"></b></div></td><td><pre>${{esc(step.summary)}}</pre></td></tr>`).join("")}}</tbody></table>`;
}}
async function load() {{
  try {{ render(await rpc("view.get")); }} catch (err) {{ $("summary").textContent = err.message || String(err); }}
}}
async function refreshAnalysisStatus() {{
  try {{
    const status = await rpc("analysis.status");
    state.analysisEnabled = !!status.enabled;
    $("analyze").disabled = !state.analysisEnabled;
    $("batch").disabled = !state.analysisEnabled;
  }} catch (_err) {{
    state.analysisEnabled = false;
  }}
}}
async function analyzeSelected() {{
  if (!state.selectedTrial) return;
  await rpc("analysis.run", {{trial_key: state.selectedTrial, overwrite: true}});
  await load();
}}
async function analyzeFailed() {{
  await rpc("analysis.batch_failed", {{overwrite: true}});
  await load();
}}
$("refresh").addEventListener("click", load);
$("analyze").addEventListener("click", analyzeSelected);
$("batch").addEventListener("click", analyzeFailed);
$("search").addEventListener("input", renderMatrix);
load();
refreshAnalysisStatus();
</script>
</body>
</html>"##,
        css = report_css(),
        token = escape_html(token),
    )
}
