use std::collections::{BTreeSet, HashMap};

use crate::gateway_now_ms;
use psychevo::host_paths::normalized_native_path;

use super::binding::BrowserSession;

pub(super) const MAX_BROWSER_SESSIONS: usize = 2_048;
pub(super) const BROWSER_SESSION_ABSOLUTE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
pub(super) const BROWSER_SESSION_IDLE_TTL_MS: i64 = 2 * 60 * 60 * 1_000;

pub(super) fn browser_session_cookie(session_id: &str, secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "psychevo_gateway_session={session_id}; Path=/; Max-Age=86400; HttpOnly; SameSite=Lax{secure_attribute}"
    )
}

#[derive(Debug)]
struct BrowserSessionEntry {
    session: BrowserSession,
    created_at_ms: i64,
    last_seen_ms: i64,
}

#[derive(Debug, Default)]
pub(super) struct BrowserSessionStore {
    entries: HashMap<String, BrowserSessionEntry>,
    workspace_preview_generations: HashMap<String, u64>,
}

impl BrowserSessionStore {
    pub(super) fn insert(
        &mut self,
        session_id: String,
        session: BrowserSession,
    ) -> Option<BrowserSession> {
        self.insert_at(session_id, session, gateway_now_ms())
    }

    pub(super) fn insert_at(
        &mut self,
        session_id: String,
        session: BrowserSession,
        now_ms: i64,
    ) -> Option<BrowserSession> {
        self.prune(now_ms);
        let previous = self.entries.remove(&session_id).map(|entry| entry.session);
        while self.entries.len() >= MAX_BROWSER_SESSIONS {
            self.evict_lru();
        }
        self.entries.insert(
            session_id,
            BrowserSessionEntry {
                session,
                created_at_ms: now_ms,
                last_seen_ms: now_ms,
            },
        );
        previous
    }

    pub(super) fn authenticate(&mut self, session_id: &str) -> Option<BrowserSession> {
        self.authenticate_at(session_id, gateway_now_ms())
    }

    pub(super) fn authenticate_at(
        &mut self,
        session_id: &str,
        now_ms: i64,
    ) -> Option<BrowserSession> {
        self.prune(now_ms);
        let entry = self.entries.get_mut(session_id)?;
        entry.last_seen_ms = now_ms;
        Some(entry.session.clone())
    }

    pub(super) fn get_mut(&mut self, session_id: &str) -> Option<&mut BrowserSession> {
        let now_ms = gateway_now_ms();
        self.prune(now_ms);
        let entry = self.entries.get_mut(session_id)?;
        entry.last_seen_ms = now_ms;
        Some(&mut entry.session)
    }

    pub(super) fn invalidate_workspace_preview_roots(&mut self, workspace_id: &str) {
        let now_ms = gateway_now_ms();
        self.prune(now_ms);
        let generation = self
            .workspace_preview_generations
            .entry(workspace_id.to_string())
            .or_default();
        *generation = generation.saturating_add(1);
        for entry in self.entries.values_mut().filter(|entry| {
            entry.session.workspace_preview_workspace_id.as_deref() == Some(workspace_id)
        }) {
            entry.session.workspace_preview_roots =
                BTreeSet::from([normalized_native_path(&entry.session.cwd)]);
            entry.session.workspace_preview_workspace_id = None;
            entry.session.workspace_preview_workspace_revision = None;
        }
    }

    pub(super) fn workspace_preview_generation(&mut self, workspace_id: &str) -> u64 {
        let now_ms = gateway_now_ms();
        self.prune(now_ms);
        *self
            .workspace_preview_generations
            .entry(workspace_id.to_string())
            .or_default()
    }

    pub(super) fn set_workspace_preview_roots_if_generation(
        &mut self,
        session_id: &str,
        workspace_id: &str,
        expected_generation: u64,
        workspace_revision: i64,
        roots: BTreeSet<std::path::PathBuf>,
    ) -> bool {
        if self.workspace_preview_generation(workspace_id) != expected_generation {
            return false;
        }
        let Some(session) = self.get_mut(session_id) else {
            return false;
        };
        session.workspace_preview_roots = roots;
        session.workspace_preview_workspace_id = Some(workspace_id.to_string());
        session.workspace_preview_workspace_revision = Some(workspace_revision);
        true
    }

    #[cfg(test)]
    pub(super) fn get(&self, session_id: &str) -> Option<&BrowserSession> {
        self.entries.get(session_id).map(|entry| &entry.session)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    fn prune(&mut self, now_ms: i64) {
        self.entries.retain(|_, entry| {
            now_ms.saturating_sub(entry.created_at_ms) < BROWSER_SESSION_ABSOLUTE_TTL_MS
                && now_ms.saturating_sub(entry.last_seen_ms) < BROWSER_SESSION_IDLE_TTL_MS
        });
    }

    fn evict_lru(&mut self) {
        let lru = self
            .entries
            .iter()
            .min_by(|(left_id, left), (right_id, right)| {
                (left.last_seen_ms, left.created_at_ms, left_id.as_str()).cmp(&(
                    right.last_seen_ms,
                    right.created_at_ms,
                    right_id.as_str(),
                ))
            })
            .map(|(session_id, _)| session_id.clone());
        if let Some(session_id) = lru {
            self.entries.remove(&session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_gateway_protocol::source::GatewaySource;
    use std::path::PathBuf;

    fn session(path: &str) -> BrowserSession {
        BrowserSession::with_external_action_grant(
            PathBuf::from(path),
            GatewaySource::new("browser-test", path).persistent(),
        )
    }

    #[test]
    fn browser_sessions_enforce_idle_and_absolute_ttl() {
        let mut store = BrowserSessionStore::default();
        store.insert_at("idle".to_string(), session("/idle"), 0);
        assert!(
            store
                .authenticate_at("idle", BROWSER_SESSION_IDLE_TTL_MS - 1)
                .is_some()
        );
        assert!(
            store
                .authenticate_at("idle", BROWSER_SESSION_IDLE_TTL_MS * 2 - 2,)
                .is_some()
        );
        assert!(
            store
                .authenticate_at("idle", BROWSER_SESSION_ABSOLUTE_TTL_MS)
                .is_none()
        );

        store.insert_at("expired-idle".to_string(), session("/idle"), 0);
        assert!(
            store
                .authenticate_at("expired-idle", BROWSER_SESSION_IDLE_TTL_MS)
                .is_none()
        );
    }

    #[test]
    fn browser_session_cap_evicts_deterministic_lru() {
        let mut store = BrowserSessionStore::default();
        for index in 0..MAX_BROWSER_SESSIONS {
            store.insert_at(
                format!("session-{index:04}"),
                session("/workspace"),
                index as i64,
            );
        }
        assert!(
            store
                .authenticate_at("session-0000", MAX_BROWSER_SESSIONS as i64)
                .is_some()
        );
        store.insert_at(
            "new-session".to_string(),
            session("/workspace"),
            MAX_BROWSER_SESSIONS as i64 + 1,
        );

        assert_eq!(store.len(), MAX_BROWSER_SESSIONS);
        assert!(store.get("session-0000").is_some());
        assert!(store.get("session-0001").is_none());
        assert!(store.get("new-session").is_some());
    }

    #[test]
    fn workspace_preview_invalidation_is_scoped_to_the_edited_workspace() {
        let mut store = BrowserSessionStore::default();
        let now_ms = gateway_now_ms();
        let mut first = session("/first");
        first.workspace_preview_workspace_id = Some("workspace-a".to_string());
        first
            .workspace_preview_roots
            .insert(PathBuf::from("/shared-a"));
        let mut second = session("/second");
        second.workspace_preview_workspace_id = Some("workspace-b".to_string());
        second
            .workspace_preview_roots
            .insert(PathBuf::from("/shared-b"));
        store.insert_at("first".to_string(), first, now_ms);
        store.insert_at("second".to_string(), second, now_ms);

        store.invalidate_workspace_preview_roots("workspace-a");

        assert_eq!(
            store.get("first").expect("first").workspace_preview_roots,
            BTreeSet::from([normalized_native_path(PathBuf::from("/first").as_path())])
        );
        assert!(
            store
                .get("second")
                .expect("second")
                .workspace_preview_roots
                .contains(&PathBuf::from("/shared-b"))
        );
    }

    #[test]
    fn stale_workspace_preview_writer_cannot_restore_invalidated_roots() {
        let mut store = BrowserSessionStore::default();
        let now_ms = gateway_now_ms();
        store.insert_at("browser".to_string(), session("/primary"), now_ms);
        let generation = store.workspace_preview_generation("workspace-a");

        store.invalidate_workspace_preview_roots("workspace-a");
        let installed = store.set_workspace_preview_roots_if_generation(
            "browser",
            "workspace-a",
            generation,
            1,
            BTreeSet::from([PathBuf::from("/primary"), PathBuf::from("/removed")]),
        );

        assert!(!installed);
        assert_eq!(
            store
                .get("browser")
                .expect("browser")
                .workspace_preview_roots,
            BTreeSet::from([normalized_native_path(PathBuf::from("/primary").as_path())])
        );
    }

    #[test]
    fn browser_cookie_matches_absolute_ttl_and_https_security() {
        assert_eq!(
            browser_session_cookie("session", false),
            "psychevo_gateway_session=session; Path=/; Max-Age=86400; HttpOnly; SameSite=Lax"
        );
        assert!(browser_session_cookie("session", true).ends_with("; Secure"));
    }
}
