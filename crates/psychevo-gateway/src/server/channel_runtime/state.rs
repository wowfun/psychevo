use super::*;

#[derive(Clone)]
pub(in crate::server) struct ChannelRuntimeState {
    inner: Arc<Mutex<ChannelRuntimeInner>>,
    status_path: PathBuf,
}

#[derive(Debug, Default)]
struct ChannelRuntimeInner {
    records: BTreeMap<String, ChannelRunnerRecord>,
    active: BTreeMap<String, CancellationToken>,
    wechat_login_grace_until_ms: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelRunnerRecord {
    state: String,
    reason: Option<String>,
    last_poll_at_ms: Option<i64>,
    last_healthy_poll_at_ms: Option<i64>,
    last_inbound_at_ms: Option<i64>,
    last_outbound_at_ms: Option<i64>,
    last_ilink_errcode: Option<i64>,
    last_error: Option<String>,
}

impl Default for ChannelRuntimeState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelRuntimeInner::default())),
            status_path: PathBuf::new(),
        }
    }
}

impl ChannelRuntimeState {
    pub(in crate::server) fn new(home: &Path) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelRuntimeInner::default())),
            status_path: home.join("gateway").join("channels-status.json"),
        }
    }

    pub(in crate::server) fn runner_view(&self, id: &str) -> wire::ChannelRunnerView {
        let record = self
            .inner
            .lock()
            .expect("channel runtime state poisoned")
            .records
            .get(id)
            .cloned()
            .unwrap_or_else(ChannelRunnerRecord::stopped);
        wire::ChannelRunnerView {
            state: record.state,
            reason: record.reason,
            last_poll_at_ms: record.last_poll_at_ms,
            last_healthy_poll_at_ms: record.last_healthy_poll_at_ms,
            last_inbound_at_ms: record.last_inbound_at_ms,
            last_outbound_at_ms: record.last_outbound_at_ms,
            last_ilink_errcode: record.last_ilink_errcode,
            last_error: record.last_error,
        }
    }

    pub(super) fn activate(&self, id: &str) -> Option<CancellationToken> {
        let mut inner = self.inner.lock().expect("channel runtime state poisoned");
        if inner.active.contains_key(id) {
            return None;
        }
        let token = CancellationToken::new();
        inner.active.insert(id.to_string(), token.clone());
        Some(token)
    }

    pub(super) fn reconcile_active(&self, desired: &std::collections::BTreeSet<String>) {
        let cancelled = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            let stale = inner
                .active
                .keys()
                .filter(|id| !desired.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            stale
                .into_iter()
                .filter_map(|id| {
                    inner.wechat_login_grace_until_ms.remove(&id);
                    inner.active.remove(&id).map(|token| (id, token))
                })
                .collect::<Vec<_>>()
        };
        for (id, token) in cancelled {
            token.cancel();
            self.update(&id, |record| {
                record.state = "stopped".to_string();
                record.reason = None;
                record.last_error = None;
            });
        }
    }

    pub(super) fn deactivate(&self, id: &str) {
        let token = self
            .inner
            .lock()
            .expect("channel runtime state poisoned")
            .active
            .remove(id);
        if let Some(token) = token {
            token.cancel();
        }
    }

    pub(in crate::server) fn restart(&self, id: &str) {
        self.deactivate(id);
    }

    pub(in crate::server) fn start_wechat_login_grace(&self, id: &str) {
        let until_ms = gateway_now_ms().saturating_add(WECHAT_LOGIN_GRACE_MS);
        let snapshot = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            inner
                .wechat_login_grace_until_ms
                .insert(id.to_string(), until_ms);
            let record = inner
                .records
                .entry(id.to_string())
                .or_insert_with(ChannelRunnerRecord::stopped);
            record.state = "running".to_string();
            record.reason = Some("qr_login_pending".to_string());
            record.last_error = None;
            record.last_ilink_errcode = None;
            inner.records.clone()
        };
        self.write_status_snapshot(snapshot);
        eprintln!(
            "channel runner grace started: id={} channel=wechat reason=qr_login_pending grace_ms={}",
            id, WECHAT_LOGIN_GRACE_MS
        );
    }

    pub(super) fn wechat_login_grace_active(&self, id: &str) -> bool {
        let now_ms = gateway_now_ms();
        self.inner
            .lock()
            .expect("channel runtime state poisoned")
            .wechat_login_grace_until_ms
            .get(id)
            .is_some_and(|until_ms| *until_ms > now_ms)
    }

    pub(super) fn clear_wechat_login_grace(&self, id: &str) {
        self.inner
            .lock()
            .expect("channel runtime state poisoned")
            .wechat_login_grace_until_ms
            .remove(id);
    }

    pub(super) fn mark_stopped(&self, id: &str) {
        if self.wechat_login_grace_active(id) {
            self.mark_wechat_qr_login_pending(id, None);
            return;
        }
        self.update(id, |record| {
            record.state = "stopped".to_string();
            record.reason = None;
            record.last_error = None;
        });
    }

    pub(super) fn mark_blocked(&self, id: &str, message: impl Into<String>) {
        self.mark_blocked_with_reason(id, None, message, None);
    }

    pub(super) fn mark_blocked_with_reason(
        &self,
        id: &str,
        reason: Option<&str>,
        message: impl Into<String>,
        ilink_errcode: Option<i64>,
    ) {
        self.clear_wechat_login_grace(id);
        self.update(id, |record| {
            record.state = "blocked".to_string();
            record.reason = reason.map(str::to_string);
            record.last_error = Some(redact_channel_error(&message.into()));
            record.last_ilink_errcode = ilink_errcode;
        });
    }

    pub(super) fn mark_running(&self, id: &str) {
        let keep_pending = self.wechat_login_grace_active(id);
        self.update(id, |record| {
            record.state = "running".to_string();
            if !keep_pending {
                record.reason = None;
                record.last_ilink_errcode = None;
            }
            record.last_error = None;
        });
    }

    pub(super) fn mark_poll(&self, id: &str, reason: Option<&str>) {
        self.clear_wechat_login_grace(id);
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = reason.map(str::to_string);
            record.last_poll_at_ms = Some(gateway_now_ms());
            record.last_healthy_poll_at_ms = record.last_poll_at_ms;
            record.last_error = None;
            record.last_ilink_errcode = None;
        });
    }

    pub(super) fn mark_wechat_qr_login_pending(&self, id: &str, ilink_errcode: Option<i64>) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("qr_login_pending".to_string());
            record.last_ilink_errcode = ilink_errcode;
            record.last_error = None;
        });
    }

    pub(super) fn mark_inbound(&self, id: &str) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("running".to_string());
            record.last_inbound_at_ms = Some(gateway_now_ms());
        });
    }

    pub(super) fn mark_outbound(&self, id: &str) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("running".to_string());
            record.last_outbound_at_ms = Some(gateway_now_ms());
            record.last_error = None;
        });
    }

    pub(super) fn mark_error(&self, id: &str, error: &dyn std::fmt::Display) {
        let message = error.to_string();
        self.update(id, |record| {
            record.state = "error".to_string();
            record.reason = Some("error".to_string());
            record.last_ilink_errcode = wechat_ilink_error_code_from_message(&message);
            record.last_error = Some(redact_channel_error(&message));
        });
    }

    fn update(&self, id: &str, mutate: impl FnOnce(&mut ChannelRunnerRecord)) {
        let snapshot = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            let record = inner
                .records
                .entry(id.to_string())
                .or_insert_with(ChannelRunnerRecord::stopped);
            mutate(record);
            inner.records.clone()
        };
        self.write_status_snapshot(snapshot);
    }

    fn write_status_snapshot(&self, records: BTreeMap<String, ChannelRunnerRecord>) {
        if self.status_path.as_os_str().is_empty() {
            return;
        }
        if let Some(parent) = self.status_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let value = json!({ "channels": records });
        let _ = std::fs::write(
            &self.status_path,
            serde_json::to_vec_pretty(&value).unwrap_or_default(),
        );
    }
}

impl ChannelRunnerRecord {
    fn stopped() -> Self {
        Self {
            state: "stopped".to_string(),
            reason: None,
            last_poll_at_ms: None,
            last_healthy_poll_at_ms: None,
            last_inbound_at_ms: None,
            last_outbound_at_ms: None,
            last_ilink_errcode: None,
            last_error: None,
        }
    }
}
