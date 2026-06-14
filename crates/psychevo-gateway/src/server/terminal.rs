use std::collections::{BTreeMap, HashMap};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::{Error, canonicalize_workdir};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{ResolvedScope, rpc_notification};

#[derive(Clone, Default)]
pub(super) struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
}

#[derive(Clone)]
struct TerminalSession {
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalManager {
    pub(super) fn start(
        &self,
        scope: &ResolvedScope,
        params: wire::TerminalStartParams,
        inherited_env: &BTreeMap<String, String>,
        out_tx: mpsc::UnboundedSender<String>,
    ) -> psychevo_runtime::Result<wire::TerminalStartResult> {
        let cwd = resolve_terminal_cwd(&scope.workdir, params.cwd.as_deref())?;
        let rows = params.rows.clamp(4, 200);
        let cols = params.cols.clamp(20, 400);
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| Error::Message(err.to_string()))?;
        let shell = default_terminal_shell(inherited_env);
        let mut command = portable_pty::CommandBuilder::new(shell);
        command.cwd(cwd.as_os_str());
        command.env("TERM", "xterm-256color");
        for (key, value) in inherited_env {
            command.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| Error::Message(err.to_string()))?;
        let pid = child.process_id();
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| Error::Message(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| Error::Message(err.to_string()))?;
        let terminal_id = Uuid::now_v7().to_string();
        let child = Arc::new(Mutex::new(child));
        let session = TerminalSession {
            child: Arc::clone(&child),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
        };
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .insert(terminal_id.clone(), session);
        spawn_terminal_reader(terminal_id.clone(), reader, out_tx.clone());
        spawn_terminal_waiter(
            terminal_id.clone(),
            Arc::clone(&child),
            self.clone(),
            out_tx,
        );
        Ok(wire::TerminalStartResult {
            terminal_id,
            cwd: cwd.display().to_string(),
            pid,
        })
    }

    pub(super) fn write(
        &self,
        params: wire::TerminalWriteParams,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let bytes = BASE64_STANDARD
            .decode(params.data_base64.as_bytes())
            .map_err(|err| Error::Message(format!("invalid terminal data: {err}")))?;
        let session = self.session(&params.terminal_id)?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| Error::Message("terminal writer is unavailable".to_string()))?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    pub(super) fn resize(
        &self,
        params: wire::TerminalResizeParams,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let session = self.session(&params.terminal_id)?;
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
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    pub(super) fn terminate(
        &self,
        params: wire::TerminalTerminateParams,
        out_tx: mpsc::UnboundedSender<String>,
    ) -> psychevo_runtime::Result<wire::TerminalMutationResult> {
        let Some(session) = self
            .sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .remove(&params.terminal_id)
        else {
            return Ok(wire::TerminalMutationResult { accepted: false });
        };
        if let Ok(mut child) = session.child.lock() {
            let _ = child.kill();
        }
        let _ = out_tx.send(rpc_notification(
            "terminal/exited",
            serde_json::to_value(wire::TerminalExitedPayload {
                terminal_id: params.terminal_id,
                exit_code: None,
                reason: "terminated".to_string(),
            })?,
        ));
        Ok(wire::TerminalMutationResult { accepted: true })
    }

    fn session(&self, terminal_id: &str) -> psychevo_runtime::Result<TerminalSession> {
        self.sessions
            .lock()
            .expect("web terminal sessions poisoned")
            .get(terminal_id)
            .cloned()
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

fn spawn_terminal_reader(
    terminal_id: String,
    mut reader: Box<dyn Read + Send>,
    out_tx: mpsc::UnboundedSender<String>,
) {
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let payload = wire::TerminalOutputPayload {
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
    out_tx: mpsc::UnboundedSender<String>,
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

fn resolve_terminal_cwd(root: &Path, cwd: Option<&str>) -> psychevo_runtime::Result<PathBuf> {
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return Ok(root.to_path_buf());
    };
    if cwd.contains('\0') {
        return Err(Error::Message("terminal cwd is invalid".to_string()));
    }
    let raw = Path::new(cwd);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let canonical = canonicalize_workdir(&candidate)?;
    if !canonical.starts_with(root) {
        return Err(Error::Message(
            "terminal cwd is outside the workspace".to_string(),
        ));
    }
    Ok(canonical)
}

fn default_terminal_shell(inherited_env: &BTreeMap<String, String>) -> String {
    inherited_env
        .get("SHELL")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| std::env::var("SHELL").ok())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| inherited_env.get("COMSPEC").cloned())
        .or_else(|| std::env::var("COMSPEC").ok())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "cmd.exe".to_string()
            } else {
                "/bin/sh".to_string()
            }
        })
}
