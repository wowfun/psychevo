#[path = "surfaces/composer_status.rs"]
mod composer_status;
#[cfg(test)]
pub(crate) use composer_status::{
    bottom_status_context_for_width, bottom_status_session_usage_segments,
};
pub(crate) use composer_status::{
    pending_input_preview_height, render_completion_popup, render_composer,
    render_pending_input_preview, render_sidebar, render_slash_menu, render_status,
};
#[path = "surfaces/panels.rs"]
mod panels;
#[cfg(test)]
pub(crate) use panels::model_info_lines;
pub(crate) use panels::render_bottom_panel;
#[path = "surfaces/help_provider.rs"]
mod help_provider;
pub(crate) use help_provider::{
    bottom_panel_row, model_detail_capabilities, model_detail_modalities, model_detail_pricing,
    model_detail_source, render_help_panel, render_provider_wizard_panel,
};
#[path = "surfaces/diff_overlay.rs"]
mod diff_overlay;
pub(crate) use diff_overlay::render_diff_overlay;
