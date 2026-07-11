import { useCallback, useEffect, useMemo, useState } from "react";
import { ExternalLink, Play, RefreshCw, Send, Square, Users } from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type { TranscriptAgentSession } from "@psychevo/components";
import type { AgentRunView, GatewayRequestScope, TeamMemberView, TeamStatusResult } from "@psychevo/protocol";
import type { RightWorkspaceTab } from "../types";

type TeamPanelProps = {
  client: GatewayClient | null;
  disabled: boolean;
  latestGatewayEvent: unknown;
  nativeActivities: RightWorkspaceTab[];
  scope: GatewayRequestScope | null;
  threadId: string | null;
  onOpenAgentSession(session: TranscriptAgentSession): void;
};

export function TeamPanel({
  client,
  disabled,
  latestGatewayEvent,
  nativeActivities,
  scope,
  threadId,
  onOpenAgentSession
}: TeamPanelProps) {
  const [result, setResult] = useState<TeamStatusResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [controlBusy, setControlBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!client || !threadId) {
      setResult(null);
      setError(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setResult(await client.request("team/status", { scope, threadId }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [client, scope, threadId]);

  useEffect(() => {
    void refresh();
  }, [refresh, latestGatewayEvent]);

  const groupedAgents = useMemo(() => groupAgentsByMember(result?.agents ?? []), [result?.agents]);
  const team = result?.team ?? null;
  const mission = result?.mission ?? null;
  const agents = result?.agents ?? [];
  const canControl = Boolean(client) && !disabled;
  const controls = result?.control ?? null;

  async function runControl(action: string, target: string | null = null, message: string | null = null) {
    if (!client || disabled) {
      return;
    }
    setControlBusy(`${action}:${target ?? "global"}`);
    setError(null);
    try {
      await client.request("agent/control", { action, target, message, scope });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setControlBusy(null);
    }
  }

  return (
    <section className="teamPanel" aria-label="Team">
      <header>
        <div>
          <h2>Team</h2>
          <p>{mission?.goal ?? team?.description ?? "No active mission."}</p>
        </div>
        <div className="rightPanelActions">
          <button aria-label="Refresh team" disabled={loading || !client || !threadId} onClick={() => void refresh()} title="Refresh" type="button">
            <RefreshCw size={14} />
          </button>
        </div>
      </header>
      <div className="teamPanelBody">
        {error && <p className="teamPanelError" role="alert">{error}</p>}
        {!threadId && <p className="teamPanelEmpty">Start or resume a session to view team status.</p>}
        {threadId && !loading && !team && !mission && agents.length === 0 && nativeActivities.length === 0 && (
          <p className="teamPanelEmpty">No team or mission is active for this session.</p>
        )}
        {(team || mission || agents.length > 0 || nativeActivities.length > 0) && (
          <>
            <div className="teamOverview" aria-label="Team overview">
              <TeamMetric label="Mission" value={mission?.status ?? "-"} />
              <TeamMetric label="Team" value={team?.teamName ?? mission?.teamName ?? "-"} />
              <TeamMetric label="Managed" value={String(agents.length)} />
              <TeamMetric label="Native" value={String(nativeActivities.length)} />
              <TeamMetric label="Cap" value={capLabel(team?.maxParallelAgents, controls?.concurrencyCap)} />
            </div>
            {(team || mission || agents.length > 0) && <div className="teamControlStrip" aria-label="Team controls">
              <span>{controls?.spawningPaused ? "Spawning paused" : "Spawning active"}</span>
              <button
                disabled={!canControl || controlBusy !== null || controls?.spawningPaused === true}
                onClick={() => void runControl("pauseSpawning")}
                type="button"
              >
                <Square size={13} />
                <span>Pause spawning</span>
              </button>
              <button
                disabled={!canControl || controlBusy !== null || controls?.spawningPaused === false}
                onClick={() => void runControl("resumeSpawning")}
                type="button"
              >
                <Play size={13} />
                <span>Resume spawning</span>
              </button>
            </div>}
            {(team || mission || agents.length > 0) && <div className="teamMemberRows" aria-label="Psychevo-managed members">
              <h3 className="teamTrackHeading">Psychevo-managed members</h3>
              {memberRows(team?.members ?? [], groupedAgents).map((row) => (
                <section className="teamMemberRow" key={row.id} aria-label={`Member ${row.id}`}>
                  <header>
                    <Users size={15} />
                    <div>
                      <h3>{row.id}</h3>
                      <p>{[
                        "Psychevo-managed",
                        row.member?.agent,
                        row.member?.runtimeRef ? `via ${runtimeRefLabel(row.member.runtimeRef)}` : "via Native",
                        row.member?.role,
                        row.member?.description
                      ].filter(Boolean).join(" · ")}</p>
                    </div>
                    <span>{row.agents.length}</span>
                  </header>
                  {row.agents.length === 0 ? (
                    <p className="teamMemberEmpty">No child session yet.</p>
                  ) : (
                    row.agents.map((agent) => (
                      <AgentRow
                        agent={agent}
                        canControl={canControl}
                        controlBusy={controlBusy}
                        key={agent.id}
                        onOpen={() => {
                          if (!agent.childSessionId) return;
                          onOpenAgentSession({
                            agentName: agent.agentName,
                            childSessionId: agent.childSessionId,
                            parentSessionId: agent.parentSessionId,
                            task: agent.task,
                            taskName: agent.taskName,
                            title: agent.taskName ?? agent.agentName
                          });
                        }}
                        onResume={() => void runControl("resume", agent.id)}
                        onSend={(message) => void runControl("send", agent.id, message)}
                        onStop={() => void runControl("stop", agent.id)}
                      />
                    ))
                  )}
                </section>
              ))}
            </div>}
            {nativeActivities.length > 0 && (
              <div className="teamMemberRows" aria-label="Runtime-native activity">
                <h3 className="teamTrackHeading">Runtime-native activity</h3>
                {nativeActivities.map((activity) => (
                  <section className="teamMemberRow" key={activity.id} aria-label={`Native activity ${activity.title}`}>
                    <header>
                      <Users size={15} />
                      <div>
                        <h3>{activity.title}</h3>
                        <p>{[
                          runtimeRefLabel(activity.runtimeRef ?? "runtime"),
                          "Runtime-native",
                          activity.runtimeStatus || "observed",
                          activity.runtimeReadOnly === false ? "Controlled upstream" : "Read-only"
                        ].join(" · ")}</p>
                      </div>
                      <span>{activity.runtimeStatus || "observed"}</span>
                    </header>
                    <div className="teamAgentRow">
                      <div>
                        <strong>Native child activity</strong>
                        <span>Owned by {runtimeRefLabel(activity.runtimeRef ?? "runtime")}; projected into a public read-only thread.</span>
                      </div>
                      <div className="teamAgentActions">
                        <button
                          aria-label={`Open ${activity.title}`}
                          disabled={!activity.threadId}
                          onClick={() => {
                            if (!activity.threadId) return;
                            onOpenAgentSession({
                              agentName: runtimeRefLabel(activity.runtimeRef ?? "runtime"),
                              childSessionId: activity.threadId,
                              parentSessionId: activity.parentThreadId ?? threadId ?? "",
                              task: "Runtime-native activity",
                              taskName: activity.title,
                              title: activity.title
                            });
                          }}
                          title="Open runtime-native child"
                          type="button"
                        >
                          <ExternalLink size={13} />
                        </button>
                      </div>
                    </div>
                  </section>
                ))}
              </div>
            )}
            {(mission?.finalSummary || team?.finalSummary) && (
              <section className="teamSummary" aria-label="Mission summary">
                <h3>Summary</h3>
                <p>{mission?.finalSummary ?? team?.finalSummary}</p>
              </section>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function runtimeRefLabel(runtimeRef: string): string {
  if (runtimeRef === "opencode") return "OpenCode";
  if (runtimeRef === "codex") return "Codex";
  if (runtimeRef === "native") return "Native";
  return runtimeRef;
}

function TeamMetric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function AgentRow({
  agent,
  canControl,
  controlBusy,
  onOpen,
  onResume,
  onSend,
  onStop
}: {
  agent: AgentRunView;
  canControl: boolean;
  controlBusy: string | null;
  onOpen(): void;
  onResume(): void;
  onSend(message: string): void;
  onStop(): void;
}) {
  const busy = controlBusy !== null;
  const canStop = canControl && !isTerminalAgentStatus(agent.status);
  return (
    <div className="teamAgentRow">
      <div>
        <strong>{agent.taskName ?? agent.agentName}</strong>
        <span>{[agent.agentName, agent.status].filter(Boolean).join(" · ")}</span>
      </div>
      <div className="teamAgentActions">
        <button aria-label={`Open ${agent.taskName ?? agent.agentName}`} disabled={!agent.childSessionId} onClick={onOpen} title="Open child session" type="button">
          <ExternalLink size={13} />
        </button>
        <button aria-label={`Stop ${agent.teamMemberId ?? agent.agentName}`} disabled={!canStop || busy} onClick={onStop} title="Stop child" type="button">
          <Square size={13} />
        </button>
        <button aria-label={`Resume ${agent.teamMemberId ?? agent.agentName}`} disabled={!canControl || busy} onClick={onResume} title="Resume child" type="button">
          <Play size={13} />
        </button>
        <button
          aria-label={`Send to ${agent.teamMemberId ?? agent.agentName}`}
          disabled={!canControl || busy}
          onClick={() => {
            const message = window.prompt("Message to child agent");
            if (message?.trim()) {
              onSend(message.trim());
            }
          }}
          title="Send message"
          type="button"
        >
          <Send size={13} />
        </button>
      </div>
    </div>
  );
}

function groupAgentsByMember(agents: AgentRunView[]): Map<string, AgentRunView[]> {
  const grouped = new Map<string, AgentRunView[]>();
  for (const agent of agents) {
    const key = agent.teamMemberId ?? agent.agentName;
    grouped.set(key, [...(grouped.get(key) ?? []), agent]);
  }
  return grouped;
}

function memberRows(members: TeamMemberView[], groupedAgents: Map<string, AgentRunView[]>): Array<{ id: string; member: TeamMemberView | null; agents: AgentRunView[] }> {
  const rows: Array<{ id: string; member: TeamMemberView | null; agents: AgentRunView[] }> = members.map((member) => ({
    id: member.id,
    member,
    agents: groupedAgents.get(member.id) ?? []
  }));
  for (const [id, agents] of groupedAgents) {
    if (!rows.some((row) => row.id === id)) {
      rows.push({ id, member: null, agents });
    }
  }
  return rows;
}

function capLabel(teamCap: number | null | undefined, runtimeCap: number | null | undefined): string {
  if (teamCap && runtimeCap) {
    return `${teamCap}/${runtimeCap}`;
  }
  if (teamCap) {
    return String(teamCap);
  }
  if (runtimeCap) {
    return String(runtimeCap);
  }
  return "-";
}

function isTerminalAgentStatus(status: string): boolean {
  return ["completed", "failed", "cancelled", "stopped"].includes(status);
}
