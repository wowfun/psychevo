pub(crate) use std::collections::{BTreeMap, BTreeSet, HashSet};
pub(crate) use std::fs;
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::Command;

pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Value, json};

pub(crate) use crate::config::{CONFIG_FILE_NAME, load_toml_config_file, write_toml_config_file};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::prompt_templates;

#[path = "skills/catalog.rs"]
mod catalog;
pub use catalog::{
    InstallOptions, ListSkillsOptions, SaveSkillBundleOptions, ScanResult, ScanVerdict,
    SelectedSkill, SkillBundle, SkillCatalog, SkillDiagnostic, SkillDiscoveryOptions,
    SkillSettings, SkillSource, SkillTarget, discover_skills, discover_skills_with_settings,
    expand_skill_prompt, list_skills_value, list_skills_value_with_options, load_skill_settings,
    resolve_skills_home, select_explicit_skills, select_skills_for_prompt, skill_context_messages,
    skill_source_display_label, skills_visible_for_prompt_with_tools,
    skills_visible_for_prompt_with_tools_and_toolsets, view_skill_value, view_skill_value_selected,
};
pub(crate) use catalog::{Skill, SkillContextFragment, format_skills_for_prompt};
#[path = "skills/management.rs"]
mod management;
pub(crate) use management::skill_context_fragments;
pub use management::{
    create_skill, delete_skill_bundle, edit_skill, install_skill, list_skill_bundles, patch_skill,
    remove_installed_skill, remove_skill, remove_skill_file, save_skill_bundle, scan_skill_path,
    set_skill_config_value, set_skill_enabled, target_skills_dir, write_installed_skill,
    write_skill_file,
};
#[path = "skills/selection_scan.rs"]
mod selection_scan;
pub(crate) use selection_scan::{find_skill, skill_prompt_visible_for_activation};
#[path = "skills/paths.rs"]
mod paths;
