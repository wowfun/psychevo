#[path = "execution/completion.rs"]
mod completion;
#[path = "execution/run_loop/helpers.rs"]
mod helpers;
#[path = "execution/run_loop.rs"]
mod run_loop;

pub(super) use helpers::{
    main_agent_input_from_sources, selected_agent_for_result, session_model_metadata,
};
#[cfg(test)]
pub(crate) use helpers::{materialize_first_use_empty_session, should_title_visible_first_turn};
pub(crate) use run_loop::run_live_internal;
