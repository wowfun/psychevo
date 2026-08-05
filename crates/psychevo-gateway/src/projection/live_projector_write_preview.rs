use std::time::Instant;

use psychevo::tool_argument_display::{WriteArgumentPreview, write_argument_preview_from_args};
use serde_json::Value;

use super::tool_helpers::{set_metadata_field, tool_event_failed};
use super::{GatewayLiveProjector, set_write_argument_preview};

impl GatewayLiveProjector {
    pub(super) fn enrich_write_preview_from_metadata(
        &mut self,
        tool_call_id: &str,
        metadata: &mut Value,
        force: bool,
    ) {
        let Some(arguments_json) = metadata
            .get("arguments_json")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return;
        };
        let force = force || serde_json::from_str::<Value>(&arguments_json).is_ok();
        if let Some(preview) = self.observe_write_preview(tool_call_id, &arguments_json, force) {
            set_write_argument_preview(metadata, &preview, "generating");
        }
    }

    pub(super) fn enrich_write_preview_for_tool_event(
        &mut self,
        tool_call_id: &str,
        value: &Value,
        metadata: &mut Value,
    ) {
        match value.get("type").and_then(Value::as_str) {
            Some("tool_call_pending") => {
                if let Some(arguments_json) = value.get("arguments_json").and_then(Value::as_str) {
                    let force = serde_json::from_str::<Value>(arguments_json).is_ok();
                    let _ = self.observe_write_preview(tool_call_id, arguments_json, force);
                }
                if let Some(preview) = self.cached_write_preview(tool_call_id) {
                    set_write_argument_preview(metadata, &preview, "generating");
                }
            }
            Some("tool_execution_start" | "tool_execution_update") => {
                if let Some(args) = value
                    .get("args")
                    .or_else(|| self.tool_args.get(tool_call_id))
                    && let Some(preview) = write_argument_preview_from_args(args)
                {
                    self.cache_write_preview(tool_call_id, preview);
                }
                if let Some(preview) = self.cached_write_preview(tool_call_id) {
                    set_write_argument_preview(metadata, &preview, "writing");
                }
            }
            Some("tool_execution_end") if !tool_event_failed(value) => {
                self.write_previews.remove(tool_call_id);
                set_metadata_field(metadata, "write_argument_preview", Value::Null);
            }
            Some("tool_execution_end") => {
                if let Some(args) = self.tool_args.get(tool_call_id)
                    && let Some(preview) = write_argument_preview_from_args(args)
                {
                    self.cache_write_preview(tool_call_id, preview);
                }
                if let Some(preview) = self.cached_write_preview(tool_call_id) {
                    let phase = match value.get("outcome").and_then(Value::as_str) {
                        Some("aborted" | "cancelled" | "interrupted") => "cancelled",
                        _ => "failed",
                    };
                    set_write_argument_preview(metadata, &preview, phase);
                }
            }
            _ => {}
        }
    }

    fn observe_write_preview(
        &mut self,
        tool_call_id: &str,
        arguments_json: &str,
        force: bool,
    ) -> Option<WriteArgumentPreview> {
        let state = self
            .write_previews
            .entry(tool_call_id.to_string())
            .or_default();
        let now = Instant::now();
        let update = if force {
            state.tracker.flush(arguments_json, now)
        } else {
            state.tracker.observe(arguments_json, now)
        };
        if let Some(preview) = update {
            state.preview = Some(preview);
        }
        state.preview.clone()
    }

    fn cached_write_preview(&self, tool_call_id: &str) -> Option<WriteArgumentPreview> {
        self.write_previews
            .get(tool_call_id)
            .and_then(|state| state.preview.clone())
    }

    fn cache_write_preview(&mut self, tool_call_id: &str, preview: WriteArgumentPreview) {
        self.write_previews
            .entry(tool_call_id.to_string())
            .or_default()
            .preview = Some(preview);
    }
}
