#[path = "composer_status/composer.rs"]
mod composer;
#[path = "composer_status/status_usage.rs"]
mod status_usage;

pub(crate) use composer::{
    pending_input_preview_height, render_completion_popup, render_composer,
    render_pending_input_preview, render_slash_menu, render_status,
};
pub(crate) use status_usage::render_sidebar;

#[cfg(test)]
pub(crate) use composer::bottom_status_context_for_width;
#[cfg(test)]
pub(crate) use status_usage::bottom_status_session_usage_segments;
