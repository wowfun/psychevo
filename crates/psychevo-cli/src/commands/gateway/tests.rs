use std::net::SocketAddr;

use super::managed::{
    ExecutableFingerprint, ManagedBindPolicy, ManagedServerState, ProcessExecutable, managed_paths,
    managed_stale_reason, managed_startup_error, managed_status, managed_status_value,
};

#[test]
fn managed_state_executable_mismatch_is_stale() {
    let state = test_state(test_fingerprint("/old/pevo", 10, 100, Some(1)), "/static");
    let expected = test_fingerprint("/new/pevo", 20, 200, Some(2));

    assert_eq!(
        managed_stale_reason(&state, true, Some(&expected), Some("/static"), None, None),
        Some("executable_fingerprint_mismatch")
    );
}

#[test]
fn old_style_managed_state_without_executable_fingerprint_is_stale() {
    let state = ManagedServerState {
        instance_id: Some("instance-a".to_string()),
        pid: 42,
        base_url: "http://127.0.0.1:1".to_string(),
        readyz_url: "http://127.0.0.1:1/readyz".to_string(),
        started_at_ms: 100,
        version: "0.1.0".to_string(),
        executable_path: None,
        executable_modified_ms: None,
        executable_size: None,
        executable_inode: None,
        static_dir: Some("/static".to_string()),
    };
    let expected = test_fingerprint("/current/pevo", 20, 200, Some(2));

    assert_eq!(
        managed_stale_reason(&state, true, Some(&expected), Some("/static"), None, None),
        Some("missing_executable_fingerprint")
    );
}

#[test]
fn old_managed_state_without_instance_id_is_stale_before_pid_checks() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let mut state = test_state(executable.clone(), "/static");
    state.instance_id = None;

    assert_eq!(
        managed_stale_reason(&state, true, Some(&executable), Some("/static"), None, None),
        Some("missing_instance_id")
    );
}

#[test]
fn managed_state_static_dir_mismatch_is_stale() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let state = test_state(executable.clone(), "/old-static");

    assert_eq!(
        managed_stale_reason(
            &state,
            true,
            Some(&executable),
            Some("/new-static"),
            None,
            None
        ),
        Some("static_dir_mismatch")
    );
}

#[test]
fn default_managed_bind_policy_uses_fixed_port_with_range() {
    let policy = ManagedBindPolicy::new(None);

    assert_eq!(
        policy.bind_addr(),
        "127.0.0.1:58080".parse::<SocketAddr>().expect("addr")
    );
    assert_eq!(policy.fallback_ports(), 19);
    assert!(policy.allows_bound_addr("127.0.0.1:58080".parse().expect("addr")));
    assert!(policy.allows_bound_addr("127.0.0.1:58099".parse().expect("addr")));
    assert!(!policy.allows_bound_addr("127.0.0.1:58100".parse().expect("addr")));
}

#[test]
fn explicit_managed_bind_policy_is_strict() {
    let policy = ManagedBindPolicy::new(Some("127.0.0.1:60000".parse().expect("addr")));

    assert_eq!(policy.fallback_ports(), 0);
    assert!(policy.allows_bound_addr("127.0.0.1:60000".parse().expect("addr")));
    assert!(!policy.allows_bound_addr("127.0.0.1:60001".parse().expect("addr")));
}

#[test]
fn managed_state_outside_default_bind_range_is_stale() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let mut state = test_state(executable.clone(), "/static");
    state.base_url = "http://127.0.0.1:1".to_string();
    state.readyz_url = "http://127.0.0.1:1/readyz".to_string();
    let policy = ManagedBindPolicy::new(None);

    assert_eq!(
        managed_stale_reason(
            &state,
            true,
            Some(&executable),
            Some("/static"),
            Some(&policy),
            None
        ),
        Some("bind_addr_mismatch")
    );
}

#[test]
fn managed_state_inside_default_bind_range_is_reusable() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let mut state = test_state(executable.clone(), "/static");
    state.base_url = "http://127.0.0.1:58099".to_string();
    state.readyz_url = "http://127.0.0.1:58099/readyz".to_string();
    let policy = ManagedBindPolicy::new(None);

    assert_eq!(
        managed_stale_reason(
            &state,
            true,
            Some(&executable),
            Some("/static"),
            Some(&policy),
            None
        ),
        None
    );
}

#[test]
fn managed_status_reports_stale_reason() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let state = test_state(executable.clone(), "/static");

    let value = managed_status_value(&state, false, Some(&executable), None);

    assert_eq!(value["running"], false);
    assert_eq!(value["stale"], true);
    assert_eq!(value["staleReason"], "pid_not_running");
}

#[tokio::test]
async fn managed_status_reports_invalid_state_without_treating_it_as_running() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = managed_paths(temp.path());
    std::fs::create_dir_all(temp.path().join("gateway")).expect("gateway dir");
    std::fs::write(temp.path().join("gateway/server.json"), "not json").expect("state");

    let value = managed_status(&paths).await.expect("status");

    assert_eq!(value["running"], false);
    assert_eq!(value["stale"], true);
    assert_eq!(value["staleReason"], "invalid_state");
}

#[tokio::test]
async fn managed_status_reports_a_free_instance_lease_as_stale() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = managed_paths(temp.path());
    std::fs::create_dir_all(temp.path().join("gateway")).expect("gateway dir");
    let state = test_state(
        test_fingerprint("/current/pevo", 20, 200, Some(2)),
        "/static",
    );
    std::fs::write(
        temp.path().join("gateway/server.json"),
        serde_json::to_vec(&serde_json::json!({
            "instanceId": state.instance_id,
            "pid": state.pid,
            "baseUrl": state.base_url,
            "readyzUrl": state.readyz_url,
            "startedAtMs": state.started_at_ms,
            "version": state.version,
            "executablePath": state.executable_path,
            "executableModifiedMs": state.executable_modified_ms,
            "executableSize": state.executable_size,
            "executableInode": state.executable_inode,
            "staticDir": state.static_dir,
        }))
        .expect("state json"),
    )
    .expect("state");

    let value = managed_status(&paths).await.expect("status");

    assert_eq!(value["running"], false);
    assert_eq!(value["stale"], true);
    assert_eq!(value["staleReason"], "instance_lease_missing");
}

#[test]
fn managed_startup_error_reports_only_the_current_launch_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let gateway_dir = temp.path().join("gateway");
    let log_path = gateway_dir.join("server.log");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir");
    let old_output = "old launch sentinel\n";
    let current_output = "error: current launch failed\n";
    std::fs::write(&log_path, format!("{old_output}{current_output}")).expect("server log");

    let error = managed_startup_error(&managed_paths(temp.path()), old_output.len() as u64, None)
        .to_string();

    assert!(error.contains("managed gateway did not become ready"));
    assert!(error.contains(current_output.trim()));
    assert!(error.contains(&log_path.display().to_string()));
    assert!(!error.contains("old launch sentinel"));
}

#[test]
fn managed_startup_error_bounds_large_current_launch_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let gateway_dir = temp.path().join("gateway");
    let log_path = gateway_dir.join("server.log");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir");
    let output = format!(
        "early startup marker\n{}\nlatest startup marker\n",
        "x".repeat(20 * 1024)
    );
    std::fs::write(&log_path, output).expect("server log");

    let error = managed_startup_error(&managed_paths(temp.path()), 0, None).to_string();

    assert!(error.contains("[earlier startup output omitted]"));
    assert!(error.contains("latest startup marker"));
    assert!(!error.contains("early startup marker"));
}

#[test]
fn deleted_process_executable_is_stale() {
    let executable = test_fingerprint("/current/pevo", 20, 200, Some(2));
    let state = test_state(executable.clone(), "/static");
    let process = ProcessExecutable {
        path: executable.path.clone(),
        inode: executable.inode,
        deleted: true,
    };

    assert_eq!(
        managed_stale_reason(
            &state,
            true,
            Some(&executable),
            Some("/static"),
            None,
            Some(&process)
        ),
        Some("process_executable_deleted")
    );
}

fn test_state(executable: ExecutableFingerprint, static_dir: &str) -> ManagedServerState {
    ManagedServerState {
        instance_id: Some("instance-a".to_string()),
        pid: 42,
        base_url: "http://127.0.0.1:1".to_string(),
        readyz_url: "http://127.0.0.1:1/readyz".to_string(),
        started_at_ms: 100,
        version: "0.1.0".to_string(),
        executable_path: Some(executable.path),
        executable_modified_ms: Some(executable.modified_ms),
        executable_size: Some(executable.size),
        executable_inode: executable.inode,
        static_dir: Some(static_dir.to_string()),
    }
}

fn test_fingerprint(
    path: &str,
    modified_ms: i64,
    size: u64,
    inode: Option<u64>,
) -> ExecutableFingerprint {
    ExecutableFingerprint {
        path: path.to_string(),
        modified_ms,
        size,
        inode,
    }
}
