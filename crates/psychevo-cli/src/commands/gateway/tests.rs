use std::net::SocketAddr;

use super::managed::{
    ExecutableFingerprint, ManagedBindPolicy, ManagedServerState, ProcessExecutable,
    managed_stale_reason, managed_status_value,
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
