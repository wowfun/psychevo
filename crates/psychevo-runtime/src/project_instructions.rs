use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::types::RunWarning;

pub(crate) const PROJECT_INSTRUCTION_MAX_BYTES: usize = 32 * 1024;
const TRUNCATION_MARKER: &str = "\n\n[truncated: project instruction budget exhausted]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectInstructionFragment {
    pub(crate) source_name: String,
    pub(crate) source_path: PathBuf,
    pub(crate) directory: PathBuf,
    pub(crate) content: String,
    pub(crate) truncated: bool,
    pub(crate) original_bytes: usize,
    pub(crate) included_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectInstructionLoad {
    pub(crate) fragments: Vec<ProjectInstructionFragment>,
    pub(crate) warnings: Vec<RunWarning>,
}

#[derive(Debug, Clone, Copy)]
struct InstructionCandidate {
    source_name: &'static str,
    relative_path: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct ClaudeCandidate {
    relative_path: &'static str,
    expected_agents_relative_path: &'static str,
    suggestion: &'static str,
}

const INSTRUCTION_CANDIDATES: &[InstructionCandidate] = &[
    InstructionCandidate {
        source_name: "AGENTS.md",
        relative_path: "AGENTS.md",
    },
    InstructionCandidate {
        source_name: ".psychevo/AGENTS.md",
        relative_path: ".psychevo/AGENTS.md",
    },
    InstructionCandidate {
        source_name: "AGENTS.local.md",
        relative_path: "AGENTS.local.md",
    },
];

const CLAUDE_CANDIDATES: &[ClaudeCandidate] = &[
    ClaudeCandidate {
        relative_path: "CLAUDE.md",
        expected_agents_relative_path: "AGENTS.md",
        suggestion: "ln -s CLAUDE.md AGENTS.md",
    },
    ClaudeCandidate {
        relative_path: "CLAUDE.local.md",
        expected_agents_relative_path: "AGENTS.local.md",
        suggestion: "ln -s CLAUDE.local.md AGENTS.local.md",
    },
    ClaudeCandidate {
        relative_path: ".claude/CLAUDE.md",
        expected_agents_relative_path: "AGENTS.md",
        suggestion: "ln -s .claude/CLAUDE.md AGENTS.md",
    },
];

pub(crate) fn load_project_instructions(workdir: &Path) -> Result<ProjectInstructionLoad> {
    let search_dirs = project_instruction_search_dirs(workdir);
    let warnings = claude_migration_warnings(&search_dirs)?;
    let fragments = load_instruction_fragments(&search_dirs)?;
    Ok(ProjectInstructionLoad {
        fragments,
        warnings,
    })
}

fn project_instruction_search_dirs(workdir: &Path) -> Vec<PathBuf> {
    let Some(root) = project_root(workdir) else {
        return vec![workdir.to_path_buf()];
    };
    let mut dirs = Vec::new();
    let mut cursor = workdir.to_path_buf();
    loop {
        dirs.push(cursor.clone());
        if cursor == root {
            break;
        }
        let Some(parent) = cursor.parent() else {
            break;
        };
        cursor = parent.to_path_buf();
    }
    dirs.reverse();
    dirs
}

fn project_root(workdir: &Path) -> Option<PathBuf> {
    workdir
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn load_instruction_fragments(search_dirs: &[PathBuf]) -> Result<Vec<ProjectInstructionFragment>> {
    let mut fragments = Vec::new();
    let mut remaining = PROJECT_INSTRUCTION_MAX_BYTES;

    'dirs: for dir in search_dirs {
        for candidate in INSTRUCTION_CANDIDATES {
            let path = dir.join(candidate.relative_path);
            let Some(bytes) = read_optional_regular_file(&path)? else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }

            let original_bytes = trimmed.len();
            let (content, included_bytes, truncated) = budget_content(trimmed, remaining);
            remaining = remaining.saturating_sub(included_bytes);
            let rendered = render_instruction_fragment(dir, &content, truncated);
            fragments.push(ProjectInstructionFragment {
                source_name: candidate.source_name.to_string(),
                source_path: path,
                directory: dir.clone(),
                content: rendered,
                truncated,
                original_bytes,
                included_bytes,
            });
            if remaining == 0 {
                break 'dirs;
            }
        }
    }

    Ok(fragments)
}

fn budget_content(text: &str, remaining: usize) -> (String, usize, bool) {
    if text.len() <= remaining {
        return (text.to_string(), text.len(), false);
    }
    let end = utf8_boundary_at_or_before(text, remaining);
    let mut content = text[..end].to_string();
    content.push_str(TRUNCATION_MARKER);
    (content, end, true)
}

fn utf8_boundary_at_or_before(text: &str, max: usize) -> usize {
    let mut end = max.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn render_instruction_fragment(directory: &Path, content: &str, _truncated: bool) -> String {
    format!(
        "# AGENTS.md instructions for {}\n\n<INSTRUCTIONS>\n{}\n</INSTRUCTIONS>",
        directory.display(),
        content
    )
}

fn claude_migration_warnings(search_dirs: &[PathBuf]) -> Result<Vec<RunWarning>> {
    let mut warnings = Vec::new();
    for dir in search_dirs {
        for candidate in CLAUDE_CANDIDATES {
            let claude_path = dir.join(candidate.relative_path);
            if !is_regular_file(&claude_path)? {
                continue;
            }
            let agents_path = dir.join(candidate.expected_agents_relative_path);
            if is_regular_file(&agents_path)? {
                continue;
            }
            warnings.push(RunWarning {
                kind: "project_instruction".to_string(),
                message: format!(
                    "Detected {}, but Psychevo only loads AGENTS-named instruction files. Create an AGENTS symlink to share these instructions.",
                    claude_path.display()
                ),
                source_path: Some(claude_path),
                suggestion: Some(candidate.suggestion.to_string()),
            });
        }
    }
    Ok(warnings)
}

fn read_optional_regular_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(Error::Message(format!(
                "failed to inspect project instruction {}: {err}",
                path.display()
            )));
        }
    }
    fs::read(path).map(Some).map_err(|err| {
        Error::Message(format!(
            "failed to read project instruction {}: {err}",
            path.display()
        ))
    })
}

fn is_regular_file(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(Error::Message(format!(
            "failed to inspect instruction file {}: {err}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_agents_files_from_root_to_workdir() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        let nested = root.join("crates/app");
        fs::create_dir_all(nested.join(".psychevo")).expect("dirs");
        fs::create_dir(root.join(".git")).expect("git");
        fs::write(root.join("AGENTS.md"), "root").expect("root");
        fs::write(nested.join(".psychevo/AGENTS.md"), "psychevo").expect("psychevo");
        fs::write(nested.join("AGENTS.local.md"), "local").expect("local");

        let loaded = load_project_instructions(&nested).expect("loaded");

        let names = loaded
            .fragments
            .iter()
            .map(|fragment| fragment.source_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["AGENTS.md", ".psychevo/AGENTS.md", "AGENTS.local.md"]
        );
        let body = loaded
            .fragments
            .iter()
            .map(|fragment| fragment.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(body.contains("root"));
        assert!(body.contains("psychevo"));
        assert!(body.contains("local"));
    }

    #[test]
    fn workdir_git_root_ignores_parent_agents() {
        let temp = tempdir().expect("temp");
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("dirs");
        fs::create_dir(child.join(".git")).expect("child git");
        fs::write(parent.join("AGENTS.md"), "upstream-only").expect("parent");
        fs::write(child.join("AGENTS.md"), "child").expect("child");

        let loaded = load_project_instructions(&child).expect("loaded");

        assert_eq!(loaded.fragments.len(), 1);
        assert!(loaded.fragments[0].content.contains("child"));
        assert!(!loaded.fragments[0].content.contains("upstream-only"));
    }

    #[test]
    fn ignores_empty_files_and_directories() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("AGENTS.md")).expect("agents dir");
        fs::create_dir(root.join(".git")).expect("git");
        fs::write(root.join("AGENTS.local.md"), "   \n").expect("empty");

        let loaded = load_project_instructions(&root).expect("loaded");

        assert!(loaded.fragments.is_empty());
    }

    #[test]
    fn truncates_to_total_budget() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir(root.join(".git")).expect("git");
        fs::write(
            root.join("AGENTS.md"),
            "x".repeat(PROJECT_INSTRUCTION_MAX_BYTES + 16),
        )
        .expect("agents");

        let loaded = load_project_instructions(&root).expect("loaded");

        assert_eq!(loaded.fragments.len(), 1);
        assert!(loaded.fragments[0].truncated);
        assert_eq!(
            loaded.fragments[0].included_bytes,
            PROJECT_INSTRUCTION_MAX_BYTES
        );
        assert!(loaded.fragments[0].content.contains("[truncated:"));
    }

    #[test]
    fn claude_files_warn_but_do_not_load() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join(".claude")).expect("dirs");
        fs::create_dir(root.join(".git")).expect("git");
        fs::write(root.join("CLAUDE.md"), "claude").expect("claude");
        fs::write(root.join("CLAUDE.local.md"), "claude local").expect("claude local");
        fs::write(root.join(".claude/CLAUDE.md"), "claude dir").expect("claude dir");

        let loaded = load_project_instructions(&root).expect("loaded");

        assert!(loaded.fragments.is_empty());
        assert_eq!(loaded.warnings.len(), 3);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.suggestion.as_deref() == Some("ln -s CLAUDE.md AGENTS.md"))
        );
    }

    #[test]
    fn claude_warning_is_suppressed_by_matching_agents_file() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir(root.join(".git")).expect("git");
        fs::write(root.join("CLAUDE.md"), "claude").expect("claude");
        fs::write(root.join("AGENTS.md"), "agents").expect("agents");

        let loaded = load_project_instructions(&root).expect("loaded");

        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.fragments.len(), 1);
        assert!(loaded.fragments[0].content.contains("agents"));
        assert!(!loaded.fragments[0].content.contains("claude"));
    }
}
