use std::collections::HashMap;
use std::time::{Duration, Instant};

use psychevo::Error;
use serde_json::json;

pub(super) const MAX_MCP_OAUTH_SESSIONS: usize = 32;
pub(super) const MCP_OAUTH_PENDING_TTL: Duration = Duration::from_secs(10 * 60);
pub(super) const MCP_OAUTH_TERMINAL_TTL: Duration = Duration::from_secs(2 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum McpOAuthSessionStatus {
    Pending,
    Persisting,
    Succeeded,
    Failed { message: String },
}

#[derive(Debug)]
struct McpOAuthSession {
    status: McpOAuthSessionStatus,
    pending_deadline: Instant,
    terminal_deadline: Option<Instant>,
    sequence: u64,
}

#[derive(Debug, Default)]
pub(super) struct McpOAuthSessionStore {
    sessions: HashMap<String, McpOAuthSession>,
    next_sequence: u64,
}

impl McpOAuthSessionStore {
    pub(super) fn admit(&mut self, session_id: String, now: Instant) -> psychevo::Result<Instant> {
        self.prune(now);
        if self.sessions.len() >= MAX_MCP_OAUTH_SESSIONS {
            let oldest_terminal = self
                .sessions
                .iter()
                .filter(|(_, session)| {
                    matches!(
                        session.status,
                        McpOAuthSessionStatus::Succeeded | McpOAuthSessionStatus::Failed { .. }
                    )
                })
                .min_by_key(|(_, session)| session.sequence)
                .map(|(session_id, _)| session_id.clone());
            if let Some(session_id) = oldest_terminal {
                self.sessions.remove(&session_id);
            } else {
                return Err(Error::structured(
                    format!("MCP OAuth session limit reached ({MAX_MCP_OAUTH_SESSIONS})"),
                    json!({
                        "kind": "mcp_oauth_overloaded",
                        "limit": MAX_MCP_OAUTH_SESSIONS,
                    }),
                ));
            }
        }
        let pending_deadline = now + MCP_OAUTH_PENDING_TTL;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.sessions.insert(
            session_id,
            McpOAuthSession {
                status: McpOAuthSessionStatus::Pending,
                pending_deadline,
                terminal_deadline: None,
                sequence,
            },
        );
        Ok(pending_deadline)
    }

    pub(super) fn status(
        &mut self,
        session_id: &str,
        now: Instant,
    ) -> Option<McpOAuthSessionStatus> {
        self.prune(now);
        self.sessions
            .get(session_id)
            .map(|session| session.status.clone())
    }

    pub(super) fn begin_persistence(&mut self, session_id: &str, now: Instant) -> bool {
        self.prune_except(now, session_id);
        let Some(session) = self.sessions.get_mut(session_id) else {
            return false;
        };
        if !matches!(session.status, McpOAuthSessionStatus::Pending) {
            return false;
        }
        if now >= session.pending_deadline {
            session.status = oauth_timeout_status();
            session.terminal_deadline = Some(session.pending_deadline + MCP_OAUTH_TERMINAL_TTL);
            return false;
        }
        session.status = McpOAuthSessionStatus::Persisting;
        true
    }

    pub(super) fn complete(
        &mut self,
        session_id: &str,
        status: McpOAuthSessionStatus,
        now: Instant,
    ) {
        self.prune_except(now, session_id);
        let Some(session) = self.sessions.get_mut(session_id) else {
            return;
        };
        let pending = matches!(session.status, McpOAuthSessionStatus::Pending);
        if !pending && !matches!(session.status, McpOAuthSessionStatus::Persisting) {
            return;
        }
        if pending && now >= session.pending_deadline {
            session.status = oauth_timeout_status();
            session.terminal_deadline = Some(session.pending_deadline + MCP_OAUTH_TERMINAL_TTL);
        } else {
            session.status = status;
            session.terminal_deadline = Some(now + MCP_OAUTH_TERMINAL_TTL);
        }
        if session
            .terminal_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.sessions.remove(session_id);
        }
    }

    pub(super) fn remove(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    fn prune(&mut self, now: Instant) {
        self.prune_except(now, "");
    }

    fn prune_except(&mut self, now: Instant, preserved_session_id: &str) {
        for (session_id, session) in &mut self.sessions {
            if session_id != preserved_session_id
                && matches!(session.status, McpOAuthSessionStatus::Pending)
                && session.pending_deadline <= now
            {
                session.status = oauth_timeout_status();
                session.terminal_deadline = Some(session.pending_deadline + MCP_OAUTH_TERMINAL_TTL);
            }
        }
        self.sessions.retain(|session_id, session| {
            session_id == preserved_session_id
                || session
                    .terminal_deadline
                    .is_none_or(|deadline| deadline > now)
        });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.sessions.len()
    }
}

fn oauth_timeout_status() -> McpOAuthSessionStatus {
    McpOAuthSessionStatus::Failed {
        message: "MCP OAuth login timed out".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_session_expires_as_a_queryable_failure_then_is_pruned() {
        let now = Instant::now();
        let mut store = McpOAuthSessionStore::default();
        let deadline = store
            .admit("session".to_string(), now)
            .expect("admit OAuth session");

        assert_eq!(
            store.status("session", deadline - Duration::from_nanos(1)),
            Some(McpOAuthSessionStatus::Pending)
        );
        assert_eq!(
            store.status("session", deadline),
            Some(McpOAuthSessionStatus::Failed {
                message: "MCP OAuth login timed out".to_string(),
            })
        );
        assert_eq!(
            store.status("session", deadline + MCP_OAUTH_TERMINAL_TTL),
            None
        );
    }

    #[test]
    fn capacity_rejects_pending_only_store_and_evicts_oldest_terminal() {
        let now = Instant::now();
        let mut store = McpOAuthSessionStore::default();
        for index in 0..MAX_MCP_OAUTH_SESSIONS {
            store
                .admit(format!("pending-{index}"), now)
                .expect("session below capacity");
        }

        let error = store
            .admit("overloaded".to_string(), now)
            .expect_err("pending-only store must reject");
        assert_eq!(
            error.structured_data(),
            Some(&json!({
                "kind": "mcp_oauth_overloaded",
                "limit": MAX_MCP_OAUTH_SESSIONS,
            }))
        );

        store.complete(
            "pending-0",
            McpOAuthSessionStatus::Succeeded,
            now + Duration::from_secs(1),
        );
        store
            .admit("replacement".to_string(), now + Duration::from_secs(2))
            .expect("terminal session is evictable");

        assert_eq!(
            store.status("pending-0", now + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            store.status("replacement", now + Duration::from_secs(2)),
            Some(McpOAuthSessionStatus::Pending)
        );
        assert_eq!(store.len(), MAX_MCP_OAUTH_SESSIONS);
    }

    #[test]
    fn credential_persistence_remains_pending_until_its_real_terminal_result() {
        let now = Instant::now();
        let mut store = McpOAuthSessionStore::default();
        let deadline = store
            .admit("session".to_string(), now)
            .expect("admit OAuth session");

        assert!(store.begin_persistence("session", deadline - Duration::from_nanos(1)));
        assert_eq!(
            store.status("session", deadline + MCP_OAUTH_TERMINAL_TTL),
            Some(McpOAuthSessionStatus::Persisting)
        );

        store.complete(
            "session",
            McpOAuthSessionStatus::Succeeded,
            deadline + MCP_OAUTH_TERMINAL_TTL,
        );
        assert_eq!(
            store.status("session", deadline + MCP_OAUTH_TERMINAL_TTL),
            Some(McpOAuthSessionStatus::Succeeded)
        );
    }
}
