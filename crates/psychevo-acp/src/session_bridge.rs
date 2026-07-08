include!("session_bridge/imports.rs");
include!("session_bridge/slash_and_status.rs");
include!("session_bridge/session_controls.rs");

#[cfg(test)]
mod mission_tests {
    use super::*;

    #[test]
    fn acp_mission_records_team_metadata_before_prompt_run() {
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
        .expect("agent");
        let session_id = SessionId::new("acp-mission");
        let session = AcpSession::new(cwd, None, Vec::new());
        agent
            .sessions
            .lock()
            .expect("sessions")
            .insert(session_id.to_string(), session.clone());

        agent
            .record_acp_mission_metadata(&session_id, &session, Some("release"), "Ship it")
            .expect("metadata");

        let runtime_session_id = agent
            .sessions
            .lock()
            .expect("sessions")
            .get(&session_id.to_string())
            .and_then(|session| session.runtime_session_id.clone())
            .expect("runtime session");
        let team = agent
            .state
            .store()
            .find_active_agent_team_run(&runtime_session_id)
            .expect("team lookup")
            .expect("team run");
        let mission = agent
            .state
            .store()
            .find_active_agent_mission_run(&runtime_session_id)
            .expect("mission lookup")
            .expect("mission run");
        assert_eq!(team.team_name, "release");
        assert_eq!(team.max_parallel_agents, 2);
        assert_eq!(mission.goal, "Ship it");
    }
}
