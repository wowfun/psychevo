use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;

const DEFAULT_ROOTS: &[&str] = &["apps", "crates", "packages", "specs", "tools"];
const IGNORED_DIRS: &[&str] = &[
    "target",
    "dist",
    "node_modules",
    "coverage",
    "test-results",
    ".local",
];

#[derive(Debug, Args)]
pub(crate) struct LargeFilesCommand {
    #[arg(long = "root")]
    roots: Vec<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 900)]
    prod_limit: usize,
    #[arg(long, default_value_t = 1200)]
    test_limit: usize,
    #[arg(long, default_value_t = 900)]
    generated_limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FileCategory {
    Generated,
    Production,
    Test,
}

impl FileCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Production => "production",
            Self::Test => "test",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct LineLimits {
    production: usize,
    test: usize,
    generated: usize,
}

impl LineLimits {
    fn limit_for(self, category: FileCategory) -> usize {
        match category {
            FileCategory::Generated => self.generated,
            FileCategory::Production => self.production,
            FileCategory::Test => self.test,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct LargeFileRow {
    category: FileCategory,
    lines: usize,
    limit: usize,
    path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct LargeFilesReport {
    roots: Vec<String>,
    limits: LineLimits,
    oversized: Vec<LargeFileRow>,
}

pub(crate) fn run(command: LargeFilesCommand, repo_root: &Path) -> Result<()> {
    let limits = LineLimits {
        production: command.prod_limit,
        test: command.test_limit,
        generated: command.generated_limit,
    };
    let roots = selected_roots(&command.roots);
    let report = inventory(repo_root, &roots, limits)?;
    if command.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    if report.oversized.is_empty() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn selected_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    if roots.is_empty() {
        DEFAULT_ROOTS.iter().map(PathBuf::from).collect()
    } else {
        roots.to_vec()
    }
}

fn inventory(repo_root: &Path, roots: &[PathBuf], limits: LineLimits) -> Result<LargeFilesReport> {
    let mut oversized = Vec::new();
    for root in roots {
        let scan_root = if root.is_absolute() {
            root.clone()
        } else {
            repo_root.join(root)
        };
        if !scan_root.exists() {
            bail!("large-file root does not exist: {}", root.display());
        }
        scan_dir(repo_root, &scan_root, limits, &mut oversized)?;
    }
    oversized.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| right.lines.cmp(&left.lines))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(LargeFilesReport {
        roots: roots.iter().map(|root| display_root(root)).collect(),
        limits,
        oversized,
    })
}

fn scan_dir(
    repo_root: &Path,
    dir: &Path,
    limits: LineLimits,
    oversized: &mut Vec<LargeFileRow>,
) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read large-file inventory dir {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("read large-file inventory entries {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type {}", path.display()))?;
        if file_type.is_dir() {
            if is_ignored_dir(&path) {
                continue;
            }
            scan_dir(repo_root, &path, limits, oversized)?;
        } else if file_type.is_file() {
            inspect_file(repo_root, &path, limits, oversized)?;
        }
    }
    Ok(())
}

fn inspect_file(
    repo_root: &Path,
    path: &Path,
    limits: LineLimits,
    oversized: &mut Vec<LargeFileRow>,
) -> Result<()> {
    let display_path = display_path(repo_root, path);
    let lines = count_lines(path)?;
    let category = category_for(&display_path);
    let limit = limits.limit_for(category);
    if lines > limit {
        oversized.push(LargeFileRow {
            category,
            lines,
            limit,
            path: display_path,
        });
    }
    Ok(())
}

fn count_lines(path: &Path) -> Result<usize> {
    let contents = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(contents.iter().filter(|byte| **byte == b'\n').count())
}

fn category_for(path: &str) -> FileCategory {
    if is_generated(path) {
        FileCategory::Generated
    } else if is_test(path) {
        FileCategory::Test
    } else {
        FileCategory::Production
    }
}

fn is_generated(path: &str) -> bool {
    path.contains("/generated/")
        || path.starts_with("generated/")
        || direct_protocol_schema_json(path)
}

fn direct_protocol_schema_json(path: &str) -> bool {
    let prefix = "packages/protocol/schema/";
    let Some(rest) = path.strip_prefix(prefix) else {
        return false;
    };
    rest.ends_with(".json") && !rest.contains('/')
}

fn is_test(path: &str) -> bool {
    path.contains("/tests/")
        || path.starts_with("tests/")
        || path.contains("/e2e/")
        || path.starts_with("e2e/")
        || path
            .rsplit('/')
            .next()
            .map(|file_name| {
                file_name.contains(".test.")
                    || (file_name.contains(".spec.") && !path.starts_with("specs/"))
            })
            .unwrap_or(false)
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}

fn display_root(root: &Path) -> String {
    normalize_slashes(root)
}

fn display_path(repo_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(repo_root).unwrap_or(path);
    normalize_slashes(relative)
}

fn normalize_slashes(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn print_report(report: &LargeFilesReport) {
    if report.oversized.is_empty() {
        println!(
            "large-file inventory: ok (production<={} test<={} generated<={})",
            report.limits.production, report.limits.test, report.limits.generated
        );
        return;
    }

    println!("large-file inventory: oversized files (category lines limit path)");
    for row in &report.oversized {
        println!(
            "{:<10} {:>6} > {:<6} {}",
            row.category.as_str(),
            row.lines,
            row.limit,
            row.path
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new(name: &str) -> Self {
            let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "psychevo-xtask-large-files-{name}-{}-{id}",
                std::process::id()
            ));
            if path.exists() {
                fs::remove_dir_all(&path).expect("clean temp root");
            }
            fs::create_dir_all(&path).expect("create temp root");
            Self { path }
        }

        fn write_lines(&self, relative: &str, lines: usize) {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, "x\n".repeat(lines)).expect("write file");
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn test_limits() -> LineLimits {
        LineLimits {
            production: 3,
            test: 4,
            generated: 2,
        }
    }

    #[test]
    fn inventory_classifies_oversized_files_by_path_shape() {
        let temp = TempRoot::new("classifies");
        temp.write_lines("apps/workbench/src/App.tsx", 4);
        temp.write_lines("apps/workbench/src/App.test.tsx", 5);
        temp.write_lines("packages/protocol/src/generated/types.ts", 3);
        temp.write_lines("packages/protocol/schema/system.json", 3);
        temp.write_lines("specs/240-pevo-web/spec.md", 5);

        let report = inventory(
            &temp.path,
            &[
                PathBuf::from("apps"),
                PathBuf::from("packages"),
                PathBuf::from("specs"),
            ],
            test_limits(),
        )
        .expect("inventory");

        let rows: Vec<_> = report
            .oversized
            .iter()
            .map(|row| (row.category, row.lines, row.limit, row.path.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    FileCategory::Generated,
                    3,
                    2,
                    "packages/protocol/schema/system.json"
                ),
                (
                    FileCategory::Generated,
                    3,
                    2,
                    "packages/protocol/src/generated/types.ts"
                ),
                (FileCategory::Production, 5, 3, "specs/240-pevo-web/spec.md"),
                (FileCategory::Production, 4, 3, "apps/workbench/src/App.tsx"),
                (FileCategory::Test, 5, 4, "apps/workbench/src/App.test.tsx"),
            ]
        );
    }

    #[test]
    fn inventory_ignores_build_and_cache_directories() {
        let temp = TempRoot::new("ignores");
        temp.write_lines("apps/workbench/src/App.tsx", 3);
        temp.write_lines("apps/workbench/target/huge.rs", 99);
        temp.write_lines("apps/workbench/dist/bundle.js", 99);
        temp.write_lines("apps/workbench/node_modules/pkg/index.js", 99);
        temp.write_lines("apps/workbench/coverage/report.js", 99);
        temp.write_lines("apps/workbench/test-results/result.txt", 99);
        temp.write_lines("apps/workbench/.local/state.txt", 99);

        let report =
            inventory(&temp.path, &[PathBuf::from("apps")], test_limits()).expect("inventory");

        assert!(report.oversized.is_empty(), "{report:?}");
    }

    #[test]
    fn json_report_contains_roots_limits_and_rows() {
        let temp = TempRoot::new("json");
        temp.write_lines("crates/example/src/lib.rs", 4);

        let report =
            inventory(&temp.path, &[PathBuf::from("crates")], test_limits()).expect("inventory");
        let json = serde_json::to_value(&report).expect("json");

        assert_eq!(json["roots"][0], "crates");
        assert_eq!(json["limits"]["production"], 3);
        assert_eq!(json["oversized"][0]["category"], "production");
        assert_eq!(json["oversized"][0]["path"], "crates/example/src/lib.rs");
    }

    #[test]
    fn selected_roots_default_to_product_source_roots() {
        assert_eq!(
            selected_roots(&[]),
            vec![
                PathBuf::from("apps"),
                PathBuf::from("crates"),
                PathBuf::from("packages"),
                PathBuf::from("specs"),
                PathBuf::from("tools"),
            ]
        );
    }

    #[test]
    fn inventory_scans_tools_with_default_roots() {
        let temp = TempRoot::new("default-tools");
        for root in selected_roots(&[]) {
            fs::create_dir_all(temp.path.join(root)).expect("create default root");
        }
        temp.write_lines("tools/doctor-fixture/src/lib.rs", 4);

        let roots = selected_roots(&[]);
        let report = inventory(&temp.path, &roots, test_limits()).expect("inventory");

        assert_eq!(report.roots.last().map(String::as_str), Some("tools"));
        assert_eq!(report.oversized.len(), 1);
        assert_eq!(report.oversized[0].path, "tools/doctor-fixture/src/lib.rs");
    }
}
