use std::process::Command;

#[test]
fn root_help_matches_compiled_feature_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_pevo"))
        .arg("--help")
        .output()
        .expect("pevo --help");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    for command in ["install", "remove", "list", "update"] {
        assert!(
            has_command(&help, command),
            "missing base command {command}"
        );
    }
    assert_eq!(has_command(&help, "acp"), cfg!(feature = "acp"));
    for command in ["run", "tui", "web", "serve", "gateway"] {
        assert_eq!(
            has_command(&help, command),
            cfg!(feature = "gateway"),
            "unexpected {command} visibility"
        );
    }
    assert_eq!(has_command(&help, "desktop"), cfg!(feature = "desktop"));
}

fn has_command(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix(command)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    })
}
