#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn open_clarify_panel(&mut self, request: ClarifyRequestEvent) {
        self.clarify_tool_args.insert(
            request.call_id.clone(),
            clarify_request_args_value(&request),
        );
        let previous_panel = match self.bottom_panel.take() {
            Some(BottomPanel::Clarify(mut panel)) => panel.restore_panel(),
            other => other,
        };
        self.bottom_panel = Some(BottomPanel::Clarify(ClarifyPanel::new(
            request,
            previous_panel,
        )));
    }

    pub(crate) fn apply_clarify_resolved(&mut self, event: ClarifyResolvedEvent) {
        let Some(BottomPanel::Clarify(mut panel)) = self.bottom_panel.take() else {
            return;
        };
        if panel.request.call_id != event.call_id {
            self.bottom_panel = Some(BottomPanel::Clarify(panel));
            return;
        }
        self.bottom_panel = panel.restore_panel();
    }

    pub(crate) fn value_with_cached_clarify_args(
        &self,
        value: &Value,
        tool_call_id: &str,
    ) -> Value {
        let args_missing = value.get("args").is_none_or(|args| {
            args.is_null() || args.as_object().is_some_and(|obj| obj.is_empty())
        });
        if !args_missing {
            return value.clone();
        }
        let Some(args) = self.clarify_tool_args.get(tool_call_id) else {
            return value.clone();
        };
        let mut merged = value.clone();
        if let Some(object) = merged.as_object_mut() {
            object.insert("args".to_string(), args.clone());
        }
        merged
    }
}
