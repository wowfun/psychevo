use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
#[cfg(feature = "native-runtime")]
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::{
    CapabilityResult, DesktopCaptureFacade, DesktopPlatformCapabilities, Rect, RegionCapture,
};

const DESKTOP_CWD_ENV: &str = "PSYCHEVO_DESKTOP_CWD";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FloatingActivation {
    activation_id: String,
    anchor: Option<Rect>,
    attachments: Vec<Value>,
    cwd: String,
}

#[cfg(feature = "native-runtime")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenWorkbenchThreadEvent {
    thread_id: String,
}

#[cfg(feature = "native-runtime")]
#[tauri::command]
pub(crate) async fn open_thread_in_workbench(
    app: AppHandle,
    thread_id: String,
) -> Result<(), String> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err("thread id is empty".to_string());
    }
    let window = app
        .get_webview_window("workbench")
        .ok_or_else(|| "Workbench window is unavailable".to_string())?;
    window.unminimize().map_err(|err| err.to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())?;
    window
        .emit(
            "desktop-open-thread",
            OpenWorkbenchThreadEvent {
                thread_id: thread_id.to_string(),
            },
        )
        .map_err(|err| err.to_string())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn desktop_fallback_cwd() -> Result<String, String> {
    Ok(desktop_fallback_cwd_value())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn desktop_platform_capabilities() -> Result<DesktopPlatformCapabilities, String> {
    Ok(DesktopCaptureFacade::detect().platform_capabilities())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn floating_initial_activation() -> Result<FloatingActivation, String> {
    Ok(current_activation())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn floating_capture_selection() -> Result<FloatingActivation, String> {
    Ok(current_activation())
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn floating_capture_region(
    bounds: Rect,
) -> Result<CapabilityResult<RegionCapture>, String> {
    Ok(capture_region(bounds))
}

#[cfg_attr(feature = "native-runtime", tauri::command)]
pub(crate) async fn floating_begin_region_picker() -> Result<CapabilityResult<Option<Rect>>, String>
{
    Ok(DesktopCaptureFacade::detect().begin_region_picker())
}

fn fake_activation() -> FloatingActivation {
    let text = env::var("PSYCHEVO_FLOATING_TEXT")
        .ok()
        .filter(|text| !text.trim().is_empty());
    activation_from_selection(
        text.as_deref(),
        env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        now_ms(),
    )
}

fn current_activation() -> FloatingActivation {
    if env::var("PSYCHEVO_FLOATING_TEXT").is_ok_and(|text| !text.trim().is_empty()) {
        return fake_activation();
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let now = now_ms();
    match DesktopCaptureFacade::detect().selection() {
        CapabilityResult::Success(selection) => activation_from_capture(
            selection.value.text.as_deref(),
            selection.value.source_app.as_deref(),
            selection.value.anchor,
            cwd,
            now,
        ),
        CapabilityResult::Failure(_) => activation_from_capture(None, None, None, cwd, now),
    }
}

fn activation_from_selection(
    text: Option<&str>,
    cwd: PathBuf,
    activation_ms: u128,
) -> FloatingActivation {
    activation_from_capture(
        text,
        Some("Floating fallback"),
        Some(Rect {
            x: 260.0,
            y: 24.0,
            width: 280.0,
            height: 28.0,
        }),
        cwd,
        activation_ms,
    )
}

fn activation_from_capture(
    text: Option<&str>,
    source_app: Option<&str>,
    anchor: Option<Rect>,
    cwd: PathBuf,
    activation_ms: u128,
) -> FloatingActivation {
    let attachments = text
        .as_ref()
        .map(|text| {
            vec![json!({
                "id": format!("selection:{activation_ms}"),
                "kind": "textSelection",
                "name": "Selected text",
                "preview": preview_text(text),
                "sourceApp": source_app.unwrap_or("Native selection"),
                "text": text,
                "visibleToModel": true
            })]
        })
        .unwrap_or_default();
    FloatingActivation {
        activation_id: format!("capsule-{activation_ms}"),
        anchor,
        attachments,
        cwd: cwd.display().to_string(),
    }
}

fn capture_region(bounds: Rect) -> CapabilityResult<RegionCapture> {
    DesktopCaptureFacade::detect().capture_region(bounds)
}

fn desktop_fallback_cwd_value() -> String {
    desktop_fallback_cwd_from_env(
        env::var(DESKTOP_CWD_ENV).ok().as_deref(),
        env::current_dir().ok(),
    )
}

fn desktop_fallback_cwd_from_env(explicit: Option<&str>, fallback: Option<PathBuf>) -> String {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    fallback
        .unwrap_or_else(|| PathBuf::from("/"))
        .display()
        .to_string()
}

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 80 {
        return normalized;
    }
    format!("{}...", normalized.chars().take(77).collect::<String>())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_collapses_whitespace_and_truncates() {
        let text = "hello\nworld ".repeat(12);
        let preview = preview_text(&text);
        assert!(preview.len() <= 83, "{preview}");
        assert!(!preview.contains('\n'));
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn fallback_activation_maps_selected_text_to_visible_attachment() {
        let activation = activation_from_selection(
            Some("hello\nfloating capsule"),
            PathBuf::from("/tmp/workspace"),
            42,
        );

        assert_eq!(activation.activation_id, "capsule-42");
        assert_eq!(activation.cwd, "/tmp/workspace");
        assert_eq!(activation.attachments.len(), 1);
        assert_eq!(activation.attachments[0]["kind"], "textSelection");
        assert_eq!(activation.attachments[0]["id"], "selection:42");
        assert_eq!(
            activation.attachments[0]["preview"],
            "hello floating capsule"
        );
        assert_eq!(activation.attachments[0]["text"], "hello\nfloating capsule");
        assert_eq!(activation.attachments[0]["visibleToModel"], true);
    }

    #[test]
    fn fake_region_capture_uses_data_url_when_supplied() {
        // SAFETY: the test serially sets and removes a process environment
        // variable before any thread observes this helper.
        unsafe {
            env::set_var(
                "PSYCHEVO_FLOATING_REGION_DATA_URL",
                "data:image/png;base64,AA==",
            );
        }
        let capture = capture_region(Rect {
            x: 0.0,
            y: 0.0,
            width: 123.0,
            height: 45.0,
        });
        unsafe {
            env::remove_var("PSYCHEVO_FLOATING_REGION_DATA_URL");
        }
        assert_eq!(
            serde_json::to_value(capture).expect("json"),
            json!({
                "ok": true,
                "value": {
                    "dataUrl": "data:image/png;base64,AA==",
                    "name": "floating-region-123x45.png"
                }
            })
        );
    }

    #[test]
    fn desktop_fallback_cwd_prefers_explicit_env_value() {
        assert_eq!(
            desktop_fallback_cwd_from_env(
                Some(" /tmp/psychevo-desktop-workspace "),
                Some(PathBuf::from("/tmp/process-cwd"))
            ),
            "/tmp/psychevo-desktop-workspace"
        );
    }

    #[test]
    fn desktop_fallback_cwd_uses_process_cwd_without_env_value() {
        assert_eq!(
            desktop_fallback_cwd_from_env(Some(" "), Some(PathBuf::from("/tmp/process-cwd"))),
            "/tmp/process-cwd"
        );
    }
}
