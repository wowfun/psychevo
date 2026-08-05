mod session_controls;
mod slash_and_status;

#[cfg(test)]
mod mission_tests {
    use std::collections::BTreeMap;

    use agent_client_protocol::schema::v2::SessionId;
    use psychevo::ThreadListQuery;
    use uuid::Uuid;

    use crate::stdio::{AcpOptions, AcpSession, PsychevoAcpAgent};

    #[tokio::test]
    async fn acp_mission_resolves_and_registers_team_before_prompt_run() {
        let root = std::env::temp_dir().join(format!("psychevo-acp-mission-{}", Uuid::now_v7()));
        let cwd = root.join("work");
        let home = root.join("home");
        std::fs::create_dir_all(cwd.join(".psychevo/agents")).expect("agents");
        std::fs::create_dir_all(cwd.join(".psychevo/teams")).expect("teams");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            cwd.join(".psychevo/agents/general.md"),
            "---\nname: general\ndescription: General agent\n---\nGeneral agent.\n",
        )
        .expect("agent");
        std::fs::write(
            cwd.join(".psychevo/teams/release.md"),
            concat!(
                "---\n",
                "name: release\n",
                "description: Release team\n",
                "leader: general\n",
                "members:\n",
                "  - id: reviewer\n",
                "    agent: general\n",
                "    role: review\n",
                "maxParallelAgents: 2\n",
                "---\n",
                "Coordinate the release.\n"
            ),
        )
        .expect("team");
        let agent = PsychevoAcpAgent::new(AcpOptions {
            home: home.clone(),
            db_path: root.join("state.db"),
            config_path: None,
            inherited_env: BTreeMap::from([(
                "PSYCHEVO_HOME".to_string(),
                home.display().to_string(),
            )]),
        })
        .await
        .expect("agent");
        let session_id = SessionId::new("acp-mission");
        let session = AcpSession::new(cwd.clone(), Vec::new());
        agent
            .sessions
            .lock()
            .expect("sessions")
            .insert(session_id.to_string(), session.clone());

        agent
            .record_acp_mission_metadata(&session_id, &session, Some("release"), "Ship it")
            .await
            .expect("metadata");

        let thread = agent
            .sessions
            .lock()
            .expect("sessions")
            .get(&session_id.to_string())
            .and_then(|session| session.thread.clone())
            .expect("mission materializes a runtime thread");
        let snapshot = thread.snapshot().await.expect("registered mission thread");
        assert_eq!(snapshot.summary.id, thread.id());
        let threads = agent
            .framework
            .list_threads(ThreadListQuery {
                cwd: Some(cwd),
                ..Default::default()
            })
            .await
            .expect("thread list");
        assert_eq!(threads.threads.len(), 1);
    }
}
