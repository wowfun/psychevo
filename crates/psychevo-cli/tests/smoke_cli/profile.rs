#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn cli_profile_create_use_and_select_home() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join(".psychevo");
    let coder_home = root.join("profiles").join("coder");

    let create = pevo_cmd(temp.path())
        .args([
            "profile",
            "create",
            "coder",
            "--description",
            "Coding profile",
            "--alias",
        ])
        .output()
        .expect("profile create");
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    assert!(coder_home.join("config.toml").is_file());
    assert!(coder_home.join(".env").is_file());
    assert!(coder_home.join("state.db").is_file());
    assert!(coder_home.join("sessions").is_dir());
    assert!(coder_home.join("logs").is_dir());
    assert!(coder_home.join("cache").is_dir());
    assert!(coder_home.join("skills").is_dir());
    assert!(coder_home.join("agents").is_dir());
    assert!(coder_home.join("profile.toml").is_file());
    let alias = temp.path().join(".local/bin/coder");
    assert!(alias.is_file());
    assert!(
        std::fs::read_to_string(alias)
            .expect("alias")
            .contains("exec pevo -p coder")
    );

    let use_profile = pevo_cmd(temp.path())
        .args(["profile", "use", "coder"])
        .output()
        .expect("profile use");
    assert!(
        use_profile.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&use_profile.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("active_profile"))
            .expect("active profile")
            .trim(),
        "coder"
    );

    let paths = pevo_cmd(temp.path())
        .args(["-p", "coder", "config", "path", "--json"])
        .output()
        .expect("config path");
    assert!(
        paths.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&paths.stderr)
    );
    let value: Value = serde_json::from_slice(&paths.stdout).expect("paths json");
    assert_eq!(value["home"], coder_home.display().to_string());
    assert_eq!(
        value["state_db"],
        coder_home.join("state.db").display().to_string()
    );

    let list = pevo_cmd(temp.path())
        .args(["profile", "list", "--json"])
        .output()
        .expect("profile list");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let value: Value = serde_json::from_slice(&list.stdout).expect("list json");
    let profiles = value["profiles"].as_array().expect("profiles");
    assert!(profiles.iter().any(|profile| {
        profile["name"] == "coder"
            && profile["active"] == true
            && profile["description"] == "Coding profile"
    }));
}

#[test]
pub(crate) fn cli_profile_clone_copies_setup_without_runtime_state() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join(".psychevo");
    let source = root.join("profiles").join("coder");
    let target = root.join("profiles").join("reviewer");

    let create = pevo_cmd(temp.path())
        .args(["profile", "create", "coder"])
        .output()
        .expect("profile create");
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    std::fs::write(source.join("config.toml"), "model = \"mock/model\"\n").expect("config");
    std::fs::write(source.join(".env"), "MOCK_KEY=test\n").expect("env");
    let skill_dir = source.join("skills").join("writer");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(skill_dir.join("SKILL.md"), "---\nname: writer\n---\n").expect("skill");
    let agent_dir = source.join("agents");
    std::fs::write(agent_dir.join("reviewer.md"), "# Reviewer\n").expect("agent");
    std::fs::write(source.join("state.db-wal"), "do not copy").expect("wal");
    std::fs::write(source.join("logs").join("session.log"), "do not copy").expect("log");

    let clone = pevo_cmd(temp.path())
        .args([
            "profile",
            "create",
            "reviewer",
            "--clone",
            "--clone-from",
            "coder",
        ])
        .output()
        .expect("profile clone");
    assert!(
        clone.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&clone.stderr)
    );

    assert_eq!(
        std::fs::read_to_string(target.join("config.toml")).expect("target config"),
        "model = \"mock/model\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.join(".env")).expect("target env"),
        "MOCK_KEY=test\n"
    );
    assert!(target.join("skills/writer/SKILL.md").is_file());
    assert!(target.join("agents/reviewer.md").is_file());
    assert!(target.join("state.db").is_file());
    assert!(!target.join("state.db-wal").exists());
    assert!(!target.join("logs/session.log").exists());
}

#[test]
pub(crate) fn cli_profile_selection_requires_existing_named_profile() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .args(["-p", "ghost", "config", "path", "--json"])
        .output()
        .expect("missing profile");
    assert!(!output.status.success());
    let stderr = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stderr.contains("profile `ghost` does not exist"),
        "{stderr}"
    );
    assert!(stderr.contains("pevo profile create ghost"), "{stderr}");
}
