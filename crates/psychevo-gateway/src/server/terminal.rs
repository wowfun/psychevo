use std::collections::{BTreeMap, HashMap};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use psychevo::{
    Error, host_paths::GitBashRuntime, host_paths::resolve_input_path, paths::canonicalize_cwd,
    process_env::ProcessEnvOptions, process_env::apply_pty_process_env,
    process_env::terminate_pty_child_tree,
};
use psychevo_gateway_protocol as wire;
use serde_json::json;
use uuid::Uuid;

use super::event_delivery::ConnectionSender;
use super::rpc_json::rpc_notification;
use super::scope_session::ResolvedScope;

pub(super) const MAX_TERMINAL_SESSIONS: usize = 64;

#[derive(Clone, Default)]
pub(super) struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSlot>>>,
}

#[derive(Clone)]
enum TerminalSlot {
    Starting { owner_id: String },
    Active(TerminalSession),
}

#[derive(Clone)]
struct TerminalSession {
    owner_id: String,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalManager {
    pub(super) fn start(
        &self,
        scope: &ResolvedScope,
        params: wire::thread_command_turn::TerminalStartParams,
        inherited_env: &BTreeMap<String, String>,
        out_tx: ConnectionSender,
    ) -> psychevo::Result<wire::thread_command_turn::TerminalStartResult> {
        let cwd = resolve_terminal_cwd(&scope.cwd, params.cwd.as_deref())?;
        let rows = params.rows.clamp(4, 200);
        let cols = params.cols.clamp(20, 400);
        let terminal_id = Uuid::now_v7().to_string();
        let owner_id = out_tx.id().to_string();
        self.reserve_start(&terminal_id, &owner_id)?;
        let result = (|| {
            let pty_system = portable_pty::native_pty_system();
            let pair = pty_system
                .openpty(portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|err| Error::Message(err.to_string()))?;
            let (shell, shell_args) = default_terminal_shell(inherited_env)?;
            let mut command = portable_pty::CommandBuilder::new(shell);
            command.args(shell_args);
            command.cwd(cwd.as_os_str());
            apply_terminal_env(&mut command, inherited_env)?;
            let child = pair
                .slave
                .spawn_command(command)
                .map_err(|err| Error::Message(err.to_string()))?;
            let pid = child.process_id();
            drop(pair.slave);
            let prepared_io = (|| {
                let reader = pair
                    .master
                    .try_clone_reader()
                    .map_err(|err| Error::Message(err.to_string()))?;
                let writer = pair
                    .master
                    .take_writer()
                    .map_err(|err| Error::Message(err.to_string()))?;
                Ok((reader, writer))
            })();
            let (reader, writer) = match prepared_io {
                Ok(prepared) => prepared,
                Err(error) => {
                    let mut child = child;
                    terminate_pty_child_tree(child.as_mut());
                    return Err(error);
                }
            };
            let child = Arc::new(Mutex::new(child));
            let session = TerminalSession {
                owner_id,
                child: Arc::clone(&child),
                master: Arc::new(Mutex::new(pair.master)),
                writer: Arc::new(Mutex::new(writer)),
            };
            self.activate(&terminal_id, session)?;
            spawn_terminal_reader(terminal_id.clone(), reader, out_tx.clone());
            spawn_terminal_waiter(
                terminal_id.clone(),
                Arc::clone(&child),
                self.clone(),
                out_tx,
            );
            Ok(wire::thread_command_turn::TerminalStartResult {
                terminal_id: terminal_id.clone(),
                cwd: cwd.display().to_string(),
                pid,
            })
        })();
        if result.is_err() {
            self.remove(&terminal_id);
        }
        result
    }

    pub(super) fn write(
        &self,
        owner_id: &str,
        params: wire::thread_command_turn::TerminalWriteParams,
    ) -> psychevo::Result<wire::thread_command_turn::TerminalMutationResult> {
        let bytes = BASE64_STANDARD
            .decode(params.data_base64.as_bytes())
            .map_err(|err| Error::Message(format!("invalid terminal data: {err}")))?;
        let session = self.session(owner_id, &params.terminal_id)?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| Error::Message("terminal writer is unavailable".to_string()))?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(wire::thread_command_turn::TerminalMutationResult { accepted: true })
    }

    pub(super) fn resize(
        &self,
        owner_id: &str,
        params: wire::thread_command_turn::TerminalResizeParams,
    ) -> psychevo::Result<wire::thread_command_turn::TerminalMutationResult> {
        let session = self.session(owner_id, &params.terminal_id)?;
        let master = session
            .master
            .lock()
            .map_err(|_| Error::Message("terminal pty is unavailable".to_string()))?;
        master
            .resize(portable_pty::PtySize {
                rows: params.rows.clamp(4, 200),
                cols: params.cols.clamp(20, 400),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| Error::Message(err.to_string()))?;
        Ok(wire::thread_command_turn::TerminalMutationResult { accepted: true })
    }

    pub(super) fn terminate(
        &self,
        params: wire::thread_command_turn::TerminalTerminateParams,
        out_tx: ConnectionSender,
    ) -> psychevo::Result<wire::thread_command_turn::TerminalMutationResult> {
        let session = {
            let mut sessions = self
                .sessions
                .lock()
                .expect("web terminal sessions poisoned");
            match sessions.get(&params.terminal_id) {
                Some(slot) if slot.owner_id() == out_tx.id() => {}
                _ => {
                    return Ok(wire::thread_command_turn::TerminalMutationResult {
                        accepted: false,
                    });
                }
            }
            match sessions.remove(&params.terminal_id) {
                Some(TerminalSlot::Active(session)) => session,
                Some(TerminalSlot::Starting { .. }) | None => {
                    return Ok(wire::thread_command_turn::TerminalMutationResult {
                        accepted: false,
                    });
                }
            }
        };
        terminate_terminal_session(&session);
        let _ = out_tx.send(rpc_notification(
            "terminal/exited",
            serde_json::to_value(wire::thread_command_turn::TerminalExitedPayload {
                terminal_id: params.terminal_id,
                exit_code: None,
                reason: "terminated".to_string(),
            })?,
        ));
        Ok(wire::thread_command_turn::TerminalMutationResult { accepted: true })
    }

    pub(super) fn terminate_owner(&self, out_tx: &ConnectionSender) -> usize {
        let sessions = {
            let mut all = self
                .sessions
                .lock()
                .expect("web terminal sessions poisoned");
            let ids = all
                .iter()
                .filter(|(_, slot)| slot.owner_id() == out_tx.id())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| all.remove(&id).map(|slot| (id, slot)))
                .collect::<Vec<_>>()
        };
        let mut terminated = 0;
        for (terminal_id, slot) in sessions {
            let TerminalSlot::Active(session) = slot else {
                continue;
            };
            terminate_terminal_session(&session);
            let _ = out_tx.send(rpc_notification(
                "terminal/exited",
                json!({
                    "terminalId": terminal_id,
                    "exitCode": null,
                    "reason": "connection_closed",
                }),
            ));
            terminated += 1;
        }
        terminated
    }

    fn activate(&self, terminal_id: &str, session: TerminalSession) -> psychevo::Result<()> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("web terminal sessions poisoned");
        match sessions.get(terminal_id) {
            Some(TerminalSlot::Starting { owner_id }) if owner_id == &session.owner_id => {
                sessions.insert(terminal_id.to_string(), TerminalSlot::Active(session));
                Ok(())
            }
            _ => {
                drop(sessions);
                terminate_terminal_session(&session);
                Err(Error::Message(
                    "terminal connection closed during startup".to_string(),
                ))
            }
        }
    }

    fn reserve_start(&self, terminal_id: &str, owner_id: &str) -> psychevo::Result<()> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("web terminal sessions poisoned");
        if sessions.len() >= MAX_TERMINAL_SESSIONS {
            return Err(terminal_overloaded());
        }
        sessions.insert(
            terminal_id.to_string(),
            TerminalSlot::Starting {
                owner_id: owner_id.to_string(),
            },
        );
        Ok(())
    }

    fn session(&self, owner_id: &str, terminal_id: &str) -> psychevo::Result<TerminalSession> {
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .get(terminal_id)
            .and_then(|slot| match slot {
                TerminalSlot::Active(session) if session.owner_id == owner_id => {
                    Some(session.clone())
                }
                _ => None,
            })
            .ok_or_else(|| Error::Message(format!("unknown terminal: {terminal_id}")))
    }

    fn remove(&self, terminal_id: &str) -> bool {
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .remove(terminal_id)
            .is_some()
    }
}

impl TerminalSlot {
    fn owner_id(&self) -> &str {
        match self {
            Self::Starting { owner_id } => owner_id,
            Self::Active(session) => &session.owner_id,
        }
    }
}

fn terminal_overloaded() -> Error {
    Error::structured(
        format!("terminal session limit reached ({MAX_TERMINAL_SESSIONS})"),
        json!({
            "kind": "terminal_overloaded",
            "limit": MAX_TERMINAL_SESSIONS,
        }),
    )
}

fn terminate_terminal_session(session: &TerminalSession) {
    if let Ok(mut child) = session.child.lock() {
        terminate_pty_child_tree(child.as_mut());
    }
}

fn spawn_terminal_reader(
    terminal_id: String,
    mut reader: Box<dyn Read + Send>,
    out_tx: ConnectionSender,
) {
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let payload = wire::thread_command_turn::TerminalOutputPayload {
                        terminal_id: terminal_id.clone(),
                        stream: "stdout".to_string(),
                        data_base64: BASE64_STANDARD.encode(&chunk[..n]),
                    };
                    if let Ok(value) = serde_json::to_value(payload) {
                        let _ = out_tx.send(rpc_notification("terminal/output", value));
                    }
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
}

fn spawn_terminal_waiter(
    terminal_id: String,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    manager: TerminalManager,
    out_tx: ConnectionSender,
) {
    thread::spawn(move || {
        loop {
            let status = {
                let Ok(mut child) = child.lock() else {
                    return;
                };
                child.try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    if manager.remove(&terminal_id) {
                        let _ = out_tx.send(rpc_notification(
                            "terminal/exited",
                            json!({
                                "terminalId": terminal_id,
                                "exitCode": status.exit_code() as i32,
                                "reason": status.signal().unwrap_or("exited")
                            }),
                        ));
                    }
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(err) => {
                    if manager.remove(&terminal_id) {
                        let _ = out_tx.send(rpc_notification(
                            "terminal/exited",
                            json!({
                                "terminalId": terminal_id,
                                "exitCode": null,
                                "reason": err.to_string()
                            }),
                        ));
                    }
                    return;
                }
            }
        }
    });
}

fn resolve_terminal_cwd(root: &Path, cwd: Option<&str>) -> psychevo::Result<PathBuf> {
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return Ok(root.to_path_buf());
    };
    if cwd.contains('\0') {
        return Err(Error::Message("terminal cwd is invalid".to_string()));
    }
    let raw = Path::new(cwd);
    let candidate = resolve_input_path(&raw.to_string_lossy(), root)?;
    let canonical = canonicalize_cwd(&candidate)?;
    if !canonical.starts_with(root) {
        return Err(Error::Message(
            "terminal cwd is outside the workspace".to_string(),
        ));
    }
    Ok(canonical)
}

fn default_terminal_shell(
    inherited_env: &BTreeMap<String, String>,
) -> psychevo::Result<(String, Vec<String>)> {
    if cfg!(windows) {
        let git_bash = GitBashRuntime::discover(inherited_env)?;
        return Ok((
            git_bash.bash.display().to_string(),
            vec!["--login".to_string(), "-i".to_string()],
        ));
    }
    Ok((
        inherited_env
            .get("SHELL")
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .or_else(|| std::env::var("SHELL").ok())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string()),
        Vec::new(),
    ))
}

fn apply_terminal_env(
    command: &mut portable_pty::CommandBuilder,
    inherited_env: &BTreeMap<String, String>,
) -> psychevo::Result<()> {
    apply_pty_process_env(command, inherited_env, ProcessEnvOptions::new(&[]))?;
    command.env("TERM", "xterm-256color");
    Ok(())
}

#[cfg(test)]
fn terminal_effective_env(
    inherited_env: &BTreeMap<String, String>,
    windows_utf8_defaults: bool,
) -> psychevo::Result<BTreeMap<String, String>> {
    let mut env = psychevo::process_env::effective_process_env(
        inherited_env,
        ProcessEnvOptions::new(&[]).with_windows_utf8_defaults(windows_utf8_defaults),
    )?;
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_env_applies_windows_utf8_defaults_without_overrides() {
        let env = terminal_effective_env(
            &BTreeMap::from([
                ("PYTHONIOENCODING".to_string(), "utf-16".to_string()),
                ("LC_CTYPE".to_string(), "C".to_string()),
                ("TERM".to_string(), "dumb".to_string()),
            ]),
            true,
        )
        .expect("terminal env");

        assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("PYTHONIOENCODING").map(String::as_str),
            Some("utf-16")
        );
        assert_eq!(env.get("LC_CTYPE").map(String::as_str), Some("C"));
        assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
    }

    #[test]
    fn sixty_fifth_start_is_rejected_before_a_terminal_is_created() {
        let manager = TerminalManager::default();
        for index in 0..MAX_TERMINAL_SESSIONS {
            manager
                .reserve_start(&format!("terminal-{index}"), "owner")
                .expect("terminal below capacity");
        }

        let error = manager
            .reserve_start("terminal-overflow", "owner")
            .expect_err("sixty-fifth terminal");

        assert_eq!(
            error.structured_data(),
            Some(&json!({
                "kind": "terminal_overloaded",
                "limit": MAX_TERMINAL_SESSIONS,
            }))
        );
        assert_eq!(
            manager.sessions.lock().expect("terminal sessions").len(),
            MAX_TERMINAL_SESSIONS
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_start_terminates_the_spawned_child() {
        let manager = TerminalManager::default();
        manager
            .reserve_start("terminal", "owner")
            .expect("starting reservation");
        assert!(manager.remove("terminal"));

        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("pty");
        let child = pair
            .slave
            .spawn_command(portable_pty::CommandBuilder::new("/bin/sh"))
            .expect("shell");
        drop(pair.slave);
        let writer = pair.master.take_writer().expect("writer");
        let child = Arc::new(Mutex::new(child));
        let session = TerminalSession {
            owner_id: "owner".to_string(),
            child: Arc::clone(&child),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
        };

        manager
            .activate("terminal", session)
            .expect_err("disconnected start");

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if child
                .lock()
                .expect("child")
                .try_wait()
                .expect("wait")
                .is_some()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "stale terminal child remained alive"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
