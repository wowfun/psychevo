#[path = "panels/model_panels.rs"]
mod model_panels;
#[path = "panels/selection_panels.rs"]
mod selection_panels;
#[path = "panels/text_edit.rs"]
mod text_edit;

pub(crate) use selection_panels::{PermissionApprovalChoice, ProviderWizardField};
