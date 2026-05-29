#[allow(unused_imports)]
use super::*;

mod acp;
mod command;
mod fake;
mod psychevo;
mod template;
mod wrapper;

pub(crate) use acp::*;
pub(crate) use command::*;
pub(crate) use fake::*;
pub(crate) use psychevo::*;
pub(crate) use template::*;
pub(crate) use wrapper::*;

pub(crate) fn run_agent(
    case: &CasePlan,
    workspace: &Path,
    logs_dir: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    match case.agent.kind {
        AgentKind::Fake => {
            if case.agent.fake.behavior == FakeBehavior::Fail {
                push_event(
                    events,
                    &case.case_id,
                    "fake_agent_noop",
                    "fake fail agent made no workspace changes",
                    json!({ "behavior": case.agent.fake.behavior }),
                );
                return Ok(());
            }
            let changed = apply_fake_pass_fixes(&case.task, workspace)?;
            push_event(
                events,
                &case.case_id,
                "fake_agent_finished",
                "fake pass agent applied deterministic workspace changes",
                json!({
                    "behavior": case.agent.fake.behavior,
                    "changed_files": changed,
                }),
            );
            Ok(())
        }
        AgentKind::Command => run_command_agent(case, workspace, logs_dir, events),
        AgentKind::Acp | AgentKind::PsychevoAcp | AgentKind::OpencodeAcp | AgentKind::HermesAcp => {
            run_acp_agent(case, workspace, logs_dir, events)
        }
        AgentKind::HumanInLoop => {
            bail!("agent kind `human-in-loop` is only supported by `peval env verify`")
        }
        AgentKind::Psychevo => {
            let prompt = task_prompt(&case.task)?;
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_started",
                "Psychevo live adapter command started",
                json!({
                    "agent": case.agent.id,
                    "task": case.task.id,
                }),
            );
            let output = run_psychevo_agent(&case.agent, &case.task.dir, workspace, &prompt)?;
            let observation = collect_psychevo_observation_output(workspace, &output);
            append_psychevo_process_events(events, &case.case_id, &observation);
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_finished",
                "Psychevo live adapter command finished",
                json!({
                    "exit_code": output.code,
                    "stdout_bytes": output.stdout.len(),
                    "stderr_bytes": output.stderr.len(),
                    "timed_out": output.timed_out,
                }),
            );
            if output.success {
                Ok(())
            } else {
                bail!("Psychevo agent `{}` failed", case.agent.id)
            }
        }
        AgentKind::Opencode => run_wrapper_agent(
            "opencode",
            &case.agent,
            &case.task,
            workspace,
            events,
            &case.case_id,
        ),
        AgentKind::Hermes => run_wrapper_agent(
            "hermes",
            &case.agent,
            &case.task,
            workspace,
            events,
            &case.case_id,
        ),
    }
}
