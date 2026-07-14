use std::fs;
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::time::Duration;

#[cfg(unix)]
use psychevo_runtime::host_process::ProcessIdentityError;
use psychevo_runtime::host_process::{
    InstanceLease, ManagedProcess, atomic_replace, instance_lease_is_held,
};

#[test]
fn instance_lease_is_exclusive_and_released_with_its_handle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("instance.lock");

    let lease = InstanceLease::acquire(&path).expect("first lease");
    assert!(instance_lease_is_held(&path).expect("held"));
    assert!(
        InstanceLease::try_acquire(&path)
            .expect("second lease")
            .is_none()
    );

    drop(lease);
    assert!(!instance_lease_is_held(&path).expect("released"));
    assert!(
        InstanceLease::try_acquire(&path)
            .expect("third lease")
            .is_some()
    );
}

#[test]
fn atomic_replace_never_leaves_partial_contents() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("state.json");
    fs::write(&path, b"old").expect("old state");

    atomic_replace(&path, b"new state").expect("replace state");

    assert_eq!(fs::read(&path).expect("new state"), b"new state");
    assert_eq!(
        fs::read_dir(temp.path())
            .expect("directory")
            .filter_map(Result::ok)
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn unrelated_process_without_owned_group_is_rejected_and_not_terminated() {
    let mut child = Command::new("sh")
        .args(["-c", "sleep 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn unrelated process");

    let error = ManagedProcess::inspect(child.id(), "instance-a").expect_err("mismatch");
    assert!(matches!(error, ProcessIdentityError::Mismatch(_)));
    assert!(child.try_wait().expect("poll").is_none());

    child.kill().expect("cleanup unrelated process");
    child.wait().expect("wait unrelated process");
}

#[cfg(unix)]
#[test]
fn owned_process_group_is_terminated_precisely() {
    use std::os::unix::process::CommandExt;

    let mut child = Command::new("sh")
        .args(["-c", "sleep 30 & wait"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn owned process group");
    let process = ManagedProcess::inspect(child.id(), "instance-a").expect("owned process");

    assert!(process.is_alive().expect("alive"));
    process.terminate_tree(1).expect("terminate group");
    assert!(
        process
            .wait_for_exit(Duration::from_secs(2))
            .expect("wait for exit")
    );
    assert!(
        process
            .wait_for_tree_exit(Duration::from_secs(2))
            .expect("wait for process tree")
    );
    child.wait().expect("reap owned process");
}

#[cfg(windows)]
#[test]
fn windows_managed_job_helper() {
    let Ok(instance_id) = std::env::var("PSYCHEVO_TEST_JOB_INSTANCE") else {
        return;
    };
    let ready =
        std::path::PathBuf::from(std::env::var_os("PSYCHEVO_TEST_JOB_READY").expect("ready path"));
    let descendant_pid = std::path::PathBuf::from(
        std::env::var_os("PSYCHEVO_TEST_JOB_DESCENDANT_PID").expect("descendant pid path"),
    );
    let _guard = psychevo_runtime::host_process::enter_managed_process_tree(&instance_id)
        .expect("enter managed Job Object");
    let script = format!(
        "$PID | Set-Content -NoNewline -Path '{}'; Start-Sleep -Seconds 60",
        descendant_pid.display()
    );
    let mut descendant = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .spawn()
        .expect("spawn managed descendant");
    let started = std::time::Instant::now();
    while !descendant_pid.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "descendant did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(ready, b"ready").expect("ready");
    let _ = descendant.wait();
}

#[cfg(windows)]
#[test]
fn windows_job_membership_and_tree_termination_are_exact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let instance_id = uuid::Uuid::now_v7().to_string();
    let ready = temp.path().join("ready");
    let descendant_pid_path = temp.path().join("descendant.pid");
    let mut helper = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "windows_managed_job_helper", "--nocapture"])
        .env("PSYCHEVO_TEST_JOB_INSTANCE", &instance_id)
        .env("PSYCHEVO_TEST_JOB_READY", &ready)
        .env("PSYCHEVO_TEST_JOB_DESCENDANT_PID", &descendant_pid_path)
        .spawn()
        .expect("spawn managed helper");
    let started = std::time::Instant::now();
    while !ready.exists() {
        if let Some(status) = helper.try_wait().expect("poll helper") {
            panic!("managed helper exited before ready: {status}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "helper did not become ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let descendant_pid = fs::read_to_string(&descendant_pid_path)
        .expect("descendant pid")
        .trim()
        .parse::<u32>()
        .expect("numeric descendant pid");
    let helper_process = ManagedProcess::inspect(helper.id(), &instance_id).expect("helper owned");
    let descendant =
        ManagedProcess::inspect(descendant_pid, &instance_id).expect("descendant owned");

    helper_process
        .terminate_tree(1)
        .expect("terminate Job Object");
    assert!(
        helper_process
            .wait_for_tree_exit(Duration::from_secs(5))
            .expect("wait for Job Object")
    );
    assert!(!descendant.is_alive().expect("descendant exited"));
    helper.wait().expect("reap helper");
}
