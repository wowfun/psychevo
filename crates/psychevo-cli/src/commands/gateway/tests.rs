use std::net::SocketAddr;

use super::managed::{
    ExecutableFingerprint, ManagedBindPolicy, ManagedServerState, ProcessExecutable,
    force_kill_pid, managed_paths, managed_stale_reason, managed_startup_error,
    managed_status_value,
};

#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

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

#[cfg(target_os = "linux")]
#[test]
fn forced_managed_stop_kills_the_exact_process_group_tree() {
    let temp = tempfile::tempdir().expect("tempdir");
    let child_pid_path = temp.path().join("child.pid");
    let mut leader = Command::new("sh");
    leader
        .arg("-c")
        .arg("trap '' TERM; sleep 60 & echo $! > \"$CHILD_PID_FILE\"; wait")
        .env("CHILD_PID_FILE", &child_pid_path)
        .process_group(0);
    let mut leader = leader.spawn().expect("spawn managed process-group leader");
    let leader_pid = leader.id();
    let cleanup = ProcessGroupCleanup(leader_pid);
    let child_pid = wait_for_test_child_pid(&child_pid_path);

    assert_eq!(
        unsafe { libc::getpgid(leader_pid as libc::pid_t) },
        leader_pid as i32
    );
    assert_eq!(
        unsafe { libc::getpgid(child_pid as libc::pid_t) },
        leader_pid as i32
    );
    force_kill_pid(leader_pid).expect("kill exact managed process group");
    let _ = leader.wait().expect("reap process-group leader");
    assert!(
        wait_for_linux_test_pid_exit(child_pid, Duration::from_secs(2)),
        "managed child {child_pid} survived process-group fallback"
    );
    std::mem::forget(cleanup);
}

#[cfg(target_os = "linux")]
struct ProcessGroupCleanup(u32);

#[cfg(target_os = "linux")]
impl Drop for ProcessGroupCleanup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL);
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_test_child_pid(path: &std::path::Path) -> u32 {
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse::<u32>()
        {
            return pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "managed process-group child did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn wait_for_linux_test_pid_exit(pid: u32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        let running = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .is_some_and(|stat| stat.split_whitespace().nth(2) != Some("Z"));
        if !running {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
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
