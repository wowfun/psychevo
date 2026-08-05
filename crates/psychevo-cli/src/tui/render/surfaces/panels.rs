#[path = "panels/agent_model.rs"]
mod agent_model;
#[path = "panels/approval_clarify.rs"]
mod approval_clarify;

pub(crate) use approval_clarify::render_bottom_panel;

#[cfg(test)]
pub(crate) use agent_model::model_info_lines;
