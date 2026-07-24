    fn write_project_skill(state: &WebState, name: &str, description: &str) {
        let dir = state
            .inner
            .cwd
            .join(".psychevo")
            .join("skills")
            .join(name);
        std::fs::create_dir_all(&dir).expect("skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description:?}\n---\n\nUse this skill.\n"),
        )
        .expect("skill");
    }

    fn runtime_user_message(text: &str, timestamp_ms: i64) -> RuntimeMessage {
        RuntimeMessage::User {
            content: vec![UserContentBlock::text(text)],
            timestamp_ms,
        }
    }

    fn runtime_assistant_message(text: &str, timestamp_ms: i64) -> RuntimeMessage {
        RuntimeMessage::Assistant {
            content: vec![psychevo_agent_core::AssistantBlock::Text {
                text: text.to_string(),
            }],
            timestamp_ms,
            finish_reason: Some("stop".to_string()),
            outcome: psychevo_ai::Outcome::Normal,
            model: Some("fake-model".to_string()),
            provider: Some("fake-provider".to_string()),
        }
    }

    fn track_snapshot(root: &Path, cwd: &Path) -> String {
        let workspace_id = psychevo_runtime::paths::workspace_snapshot_id(cwd).expect("workspace id");
        let git_dir = root.join("workspaces").join(workspace_id);
        std::fs::create_dir_all(&git_dir).expect("snapshot git dir");
        if !git_dir.join("HEAD").exists() {
            let init = std::process::Command::new("git")
                .env("GIT_DIR", &git_dir)
                .env("GIT_WORK_TREE", cwd)
                .arg("init")
                .output()
                .expect("snapshot init");
            assert!(
                init.status.success(),
                "snapshot init failed: {}",
                String::from_utf8_lossy(&init.stderr)
            );
        }
        let add = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(cwd)
            .args(["add", "--all", "--", "."])
            .output()
            .expect("snapshot add");
        assert!(
            add.status.success(),
            "snapshot add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let tree = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(cwd)
            .arg("write-tree")
            .output()
            .expect("snapshot write-tree");
        assert!(
            tree.status.success(),
            "snapshot write-tree failed: {}",
            String::from_utf8_lossy(&tree.stderr)
        );
        let hash = String::from_utf8_lossy(&tree.stdout).trim().to_string();
        assert!(!hash.is_empty(), "snapshot hash should not be empty");
        hash
    }

    fn git<I, S>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
