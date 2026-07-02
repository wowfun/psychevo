mod api;
mod contributions;
mod install;
mod manifest;
mod marketplace;
mod records;
mod store;
mod types;
mod util;
mod worker;

pub use api::{
    plugin_doctor_value, plugin_list_value, plugin_set_enabled_value, plugin_uninstall_value,
    plugin_view_value,
};
pub(crate) use contributions::{
    load_enabled_plugin_contributions, load_enabled_plugin_hook_sources,
};
pub use install::{install_plugin, plugin_install_value};
pub use manifest::load_plugin_manifest;
pub use marketplace::{
    plugin_marketplace_add_value, plugin_marketplace_list_value, plugin_marketplace_remove_value,
};
pub use types::{
    LoadedPluginManifest, PluginDiagnostic, PluginInstallOptions, PluginInstallRecord,
    PluginInterfaceMetadata, PluginManifestKind, PluginMarketplaceEntry, PluginScope,
    PluginWorkerSpec,
};

#[cfg(test)]
pub(crate) use store::PluginStore;
#[cfg(test)]
pub(crate) use worker::{PluginWorkerTool, WorkerToolDescriptor, call_worker_tool, worker_tools};

#[cfg(test)]
mod tests;
