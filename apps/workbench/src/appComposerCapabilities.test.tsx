// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { agentRecord, gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench capabilities management", () => {
  it("opens the top-level Capabilities view and composes domain tabs", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/list")).toBe(true);
    });
    expect(within(region).getByRole("tab", { name: "Agents" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "Skills" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "Plugins" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "MCP" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "Tools" })).toBeTruthy();
    const reviewSkill = within(region).getByRole("button", { name: "Skill review" });
    expect(reviewSkill).toBeTruthy();
    expect(within(reviewSkill).getByTitle("Review code changes")).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Prompt" })).toBeNull();
    expect(within(region).queryByText("Prompt")).toBeNull();
    expect(within(region).queryByRole("button", { name: "All" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Enabled" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Disabled" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Collision" })).toBeNull();

    fireEvent.change(within(region).getByLabelText("Search Skills"), { target: { value: "bitmap" } });
    expect(within(region).queryByRole("button", { name: "Skill review" })).toBeNull();
    expect(within(region).getByRole("button", { name: "Skill imagegen" })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "Plugins" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "plugin/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /Browser/i })).toBeTruthy();
    expect(await within(region).findByRole("button", { name: /writer-kit/i })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "MCP" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "mcp/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /docs/i })).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Test" })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "Tools" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "tool/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /web/i })).toBeTruthy();
    expect(within(region).queryByRole("tab", { name: "All" })).toBeNull();
  });

  it("manages Project/Profile Markdown agent definitions", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    gatewayMock.agentRecords = [
      {
        ...agentRecord("project-helper", ["subagent"]),
        instructions: "Project helper instructions.",
        rawMarkdown: "---\nname: project-helper\ndescription: project-helper agent\n---\nProject helper instructions."
      }
    ];
    gatewayMock.shadowedAgentRecords = [
      {
        ...agentRecord("shadowed-helper", ["subagent"]),
        description: "Shadowed profile helper",
        source: "profile",
        sourceLabel: "Profile",
        target: "profile",
        path: "/tmp/profile/agents/shadowed-helper.md"
      }
    ];
    gatewayMock.disabledAgentRecords = [
      {
        ...agentRecord("disabled-helper", ["subagent"]),
        description: "Disabled profile helper",
        enabled: false,
        source: "profile",
        sourceLabel: "Profile",
        target: "profile",
        path: "/tmp/profile/agents/disabled-helper.md"
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));

    const projectHelper = await within(region).findByRole("button", { name: "Agent project-helper" });
    expect(within(projectHelper).getByTitle("project-helper agent")).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Agent shadowed-helper" })).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Agent disabled-helper" })).toBeTruthy();
    expect(within(region).getByText("Shadowed")).toBeTruthy();
    expect(within(region).getByText("Disabled")).toBeTruthy();

    fireEvent.click(within(region).getByRole("switch", { name: "Enable disabled-helper" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/setEnabled",
        params: expect.objectContaining({
          name: "disabled-helper",
          target: "profile",
          enabled: true
        })
      });
    });

    fireEvent.click(within(region).getByRole("button", { name: "Create agent" }));
    const form = await within(region).findByRole("form", { name: "Agent definition" });
    fireEvent.change(within(form).getByLabelText("Agent target"), { target: { value: "profile" } });
    fireEvent.change(within(form).getByLabelText("Agent name"), { target: { value: "reviewer" } });
    fireEvent.change(within(form).getByLabelText("Agent description"), { target: { value: "Review code changes" } });
    fireEvent.change(within(form).getByLabelText("Agent instructions"), { target: { value: "Review the diff." } });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/write",
        params: expect.objectContaining({
          name: "reviewer",
          target: "profile",
          description: "Review code changes",
          instructions: "Review the diff.",
          rawMarkdown: null
        })
      });
    });
    const reviewer = await within(region).findByRole("button", { name: "Agent reviewer" });
    fireEvent.click(reviewer);
    const agentPreview = await within(region).findByLabelText("Agent Markdown preview");
    await waitFor(() => {
      expect(within(agentPreview).getByRole("table", { name: "YAML frontmatter" })).toBeTruthy();
      expect((within(agentPreview).getByRole("button", { name: "Edit reviewer Markdown" }) as HTMLButtonElement).disabled).toBe(false);
    });
    expect(within(agentPreview).getByText("Review code changes")).toBeTruthy();
    fireEvent.click(within(agentPreview).getByRole("button", { name: "Edit reviewer Markdown" }));
    const editForm = await within(region).findByRole("form", { name: "Agent definition" });
    expect((within(editForm).getByLabelText("Agent name") as HTMLInputElement).disabled).toBe(true);
    expect((within(editForm).getByLabelText("Agent target") as HTMLSelectElement).disabled).toBe(true);
    fireEvent.click(within(editForm).getByRole("tab", { name: "Markdown" }));
    fireEvent.change(within(editForm).getByLabelText("Agent Markdown"), {
      target: { value: "---\nname: reviewer\ndescription: Review code changes\nenabled: true\n---\nReview in Markdown mode." }
    });
    fireEvent.click(within(editForm).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/write",
        params: expect.objectContaining({
          name: "reviewer",
          target: "profile",
          rawMarkdown: expect.stringContaining("Review in Markdown mode.")
        })
      });
    });

    fireEvent.click(await within(region).findByRole("button", { name: "Agent reviewer" }));
    fireEvent.click(within(region).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith("Delete agent reviewer?");
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/delete",
        params: expect.objectContaining({
          name: "reviewer",
          target: "profile"
        })
      });
    });
    confirm.mockRestore();
  });

  it("manages Project/Profile Markdown team definitions", async () => {
    gatewayMock.teamRecords = [
      {
        name: "release",
        description: "Coordinate release",
        enabled: true,
        source: "project",
        sourceLabel: "Project",
        target: "project",
        mutable: true,
        path: "/tmp/project/.psychevo/teams/release.md",
        leader: "general",
        members: [{
          id: "researcher",
          agent: "general",
          role: "research",
          runtimeRef: "codex",
          runtimeOptions: { mode: "auto-review", effort: "high" },
          runtimeProfileRevision: "18446744073709551614"
        }],
        maxParallelAgents: 4,
        diagnostics: [],
        instructions: "Ship carefully.",
        rawMarkdown: "---\nname: release\ndescription: Coordinate release\nleader: general\n---\nShip carefully."
      }
    ];
    gatewayMock.disabledTeamRecords = [
      {
        name: "disabled-team",
        description: "Paused team",
        enabled: false,
        source: "profile",
        sourceLabel: "Profile",
        target: "profile",
        mutable: true,
        path: "/tmp/profile/teams/disabled-team.md",
        leader: "general",
        members: [{ id: "tester", agent: "general" }],
        maxParallelAgents: 2,
        diagnostics: [],
        instructions: "Verify carefully.",
        rawMarkdown: "---\nname: disabled-team\ndescription: Paused team\nleader: general\n---\nVerify carefully."
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Teams" }));

    expect(await within(region).findByRole("button", { name: "Team release" })).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Team disabled-team" })).toBeTruthy();
    fireEvent.click(await within(region).findByRole("button", { name: "Edit release Markdown" }));
    const editForm = await within(region).findByRole("form", { name: "Team definition" });
    expect((within(editForm).getByLabelText("Team members") as HTMLTextAreaElement).value).toBe(
      "researcher: general | role=research | runtime=codex | revision=18446744073709551614 | option.effort=high | option.mode=auto-review"
    );
    fireEvent.click(within(editForm).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "team/write",
        params: expect.objectContaining({
          name: "release",
          members: [expect.objectContaining({
            id: "researcher",
            agent: "general",
            runtimeRef: "codex",
            runtimeOptions: { effort: "high", mode: "auto-review" },
            runtimeProfileRevision: "18446744073709551614"
          })]
        })
      });
    });
    fireEvent.click(within(region).getByRole("switch", { name: "Enable disabled-team" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "team/setEnabled",
        params: expect.objectContaining({
          name: "disabled-team",
          target: "profile",
          enabled: true
        })
      });
    });

    fireEvent.click(within(region).getByRole("button", { name: "Create team" }));
    const form = await within(region).findByRole("form", { name: "Team definition" });
    fireEvent.change(within(form).getByLabelText("Team target"), { target: { value: "profile" } });
    fireEvent.change(within(form).getByLabelText("Team name"), { target: { value: "ship" } });
    fireEvent.change(within(form).getByLabelText("Team description"), { target: { value: "Ship feature" } });
    fireEvent.change(within(form).getByLabelText("Team leader"), { target: { value: "general" } });
    fireEvent.change(within(form).getByLabelText("Team members"), { target: { value: "reviewer: general | review\ntester: general | verify | run tests | 2" } });
    fireEvent.change(within(form).getByLabelText("Team instructions"), { target: { value: "Coordinate implementation and verification." } });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "team/write",
        params: expect.objectContaining({
          name: "ship",
          target: "profile",
          description: "Ship feature",
          leader: "general",
          rawMarkdown: null,
          members: expect.arrayContaining([
            expect.objectContaining({ id: "reviewer", agent: "general", role: "review" }),
            expect.objectContaining({ id: "tester", agent: "general", role: "verify", maxTurns: 2 })
          ])
        })
      });
    });
  });

  it("manages Runtime Profiles from Agents capabilities", async () => {
    gatewayMock.runtimeProfileRecords = gatewayMock.runtimeProfileRecords.map((profile) => profile.id === "opencode" ? {
      ...profile,
      backendRef: null,
      provenance: "Direct",
      profileRevision: "11",
      capabilityRevision: "12",
      defaultModel: "openai/gpt-5",
      approvalMode: "on-request",
      workspaceRoots: ["/tmp/project"],
      envKeys: ["OPENCODE_CONFIG"],
      health: { status: "needsAuth", summary: "Authentication required", commandPath: "/usr/bin/opencode", checkedAtMs: 1_700_000_000_000 },
      readinessStages: [
        { id: "executable", status: "ready", summary: "opencode found", observedAtMs: 1_700_000_000_000 },
        { id: "authentication", status: "needsAuth", summary: "Run opencode auth login", observedAtMs: 1_700_000_000_000 }
      ]
    } : profile);
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));

    const opencode = await within(region).findByRole("button", { name: "Runtime Profile opencode" });
    expect(within(opencode).getByText("OpenCode")).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Runtime Profile native" })).toBeTruthy();

    fireEvent.click(opencode);
    const detail = await within(region).findByRole("complementary", { name: "Runtime Profile detail" });
    expect(within(detail).getByText("opencode serve")).toBeTruthy();
    expect(within(detail).getAllByText("build")).toHaveLength(2);
    expect(within(detail).getByText("Direct · Generated · Needs auth")).toBeTruthy();
    expect(within(detail).getByText("Run opencode auth login")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/session/list")).toBe(false);
    fireEvent.click(within(detail).getByRole("button", { name: "Load sessions" }));
    expect(await within(detail).findByText("This Runtime Profile does not expose native session history.")).toBeTruthy();

    fireEvent.click(within(detail).getByRole("button", { name: "Refresh Catalog" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/snapshot",
        params: expect.objectContaining({ runtimeRef: "opencode" })
      });
    });
    fireEvent.click(within(detail).getByRole("button", { name: "Repair auth" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/auth/action",
        params: expect.objectContaining({ runtimeRef: "opencode", action: "repair" })
      });
    });
    expect((await within(detail).findAllByText(/opencode auth login/)).length).toBeGreaterThan(1);

    fireEvent.click(within(region).getByRole("button", { name: "Runtime Profile codex" }));
    fireEvent.click(within(detail).getByRole("button", { name: "Repair auth" }));
    const startLogin = await within(detail).findByRole("button", { name: "Start managed login" });
    fireEvent.click(startLogin);
    const loginLink = await within(detail).findByRole("link", { name: "Open login" });
    expect(loginLink.getAttribute("href")).toBe("https://auth.example.test/codex");
    expect(within(detail).getByText("CODE-123")).toBeTruthy();
    fireEvent.click(within(detail).getByRole("button", { name: "Cancel login" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/auth/action",
        params: expect.objectContaining({
          runtimeRef: "codex",
          action: "cancel",
          input: { loginId: "login-fixture" }
        })
      });
    });
    fireEvent.click(opencode);

    fireEvent.click(within(region).getByRole("switch", { name: "Disable opencode" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/profile/setEnabled",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false
        })
      });
    });

    fireEvent.click(within(detail).getByRole("button", { name: "Doctor" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/health/check",
        params: expect.objectContaining({ runtimeRef: "opencode" })
      });
    });
    expect((await within(detail).findAllByText("OpenCode is available")).length).toBeGreaterThan(0);
  });

  it("creates, edits, and deletes structured Runtime Profile configurations", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const codex = gatewayMock.runtimeProfileRecords.find((profile) => profile.id === "codex");
    if (!codex) throw new Error("expected Codex Runtime Profile fixture");
    gatewayMock.runtimeProfileRecords = [
      ...gatewayMock.runtimeProfileRecords,
      {
        ...codex,
        id: "review-codex",
        label: "Review Codex",
        generated: false,
        configured: true,
        sourceTargets: ["project"],
        envKeys: ["CODEX_HOME"]
      },
      {
        ...codex,
        id: "acp:cursor",
        label: "Cursor (ACP)",
        runtime: "acp",
        backendRef: "cursor",
        provenance: "ACP",
        generated: true,
        configured: false,
        sourceTargets: []
      }
    ];
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));

    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile acp:cursor" }));
    let detail = await within(region).findByRole("complementary", { name: "Runtime Profile detail" });
    expect((within(detail).getByRole("button", { name: "Customize" }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(detail).getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(within(region).getByRole("button", { name: "Create profile" }));
    let form = within(detail).getByRole("form", { name: "Runtime Profile" });
    fireEvent.change(within(form).getByLabelText("Runtime Profile target"), { target: { value: "profile" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile id"), { target: { value: "cursor-review" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile runtime"), { target: { value: "acp" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile label"), { target: { value: "Cursor Review" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile ACP backend ref"), { target: { value: "cursor" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile arguments"), { target: { value: "--stdio\n--verbose" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile environment"), { target: { value: "CURSOR_TRACE=1" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile workspace roots"), { target: { value: "/tmp/project\n/tmp/shared" } });
    fireEvent.change(within(form).getByLabelText("Runtime Profile options"), { target: { value: "{\"trace\":true}" } });
    fireEvent.click(within(form).getByRole("button", { name: "Create profile" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/profile/write",
        params: expect.objectContaining({
          id: "cursor-review",
          target: "profile",
          runtime: "acp",
          label: "Cursor Review",
          backendRef: "cursor",
          args: ["--stdio", "--verbose"],
          env: { CURSOR_TRACE: "1" },
          workspaceRoots: ["/tmp/project", "/tmp/shared"],
          options: { trace: true }
        })
      });
    });

    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile review-codex" }));
    detail = within(region).getByRole("complementary", { name: "Runtime Profile detail" });
    fireEvent.click(within(detail).getByRole("button", { name: "Edit" }));
    form = await within(detail).findByRole("form", { name: "Runtime Profile" });
    expect((within(form).getByLabelText("Runtime Profile target") as HTMLSelectElement).disabled).toBe(true);
    expect((within(form).getByLabelText("Runtime Profile id") as HTMLInputElement).disabled).toBe(true);
    expect(within(form).getByText(/Stored values for CODEX_HOME are hidden/)).toBeTruthy();
    fireEvent.change(within(form).getByLabelText("Runtime Profile label"), { target: { value: "Review Codex Updated" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save changes" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/profile/write",
        params: expect.objectContaining({
          id: "review-codex",
          target: "project",
          runtime: "codex",
          label: "Review Codex Updated",
          env: {}
        })
      });
    });

    await waitFor(() => expect(within(region).getAllByText("Review Codex Updated").length).toBeGreaterThan(0));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile review-codex" }));
    detail = within(region).getByRole("complementary", { name: "Runtime Profile detail" });
    fireEvent.click(within(detail).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/profile/delete",
        params: expect.objectContaining({ id: "review-codex", target: "project" })
      });
    });
    expect(confirm).toHaveBeenCalledWith("Delete the Project configuration for Review Codex Updated?");
    confirm.mockRestore();
  });

  it("shows native session ownership, fidelity, and capability actions by Profile and cwd", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("native-session-1", "Native review")];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile native" }));

    const sessions = await within(region).findByRole("region", { name: "Native Sessions" });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/session/list")).toBe(false);
    fireEvent.click(within(sessions).getByRole("button", { name: "Load sessions" }));
    expect(await within(sessions).findByText("Native review")).toBeTruthy();
    expect(within(sessions).getByText(/Read-write · Full history/)).toBeTruthy();
    expect(within(sessions).getByText("/tmp/project")).toBeTruthy();
    fireEvent.click(within(sessions).getByRole("button", { name: "Resume" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/resume",
        params: expect.objectContaining({
          runtimeRef: "native",
          sessionHandle: "native-session-1"
        })
      });
    });
  });

  it("offers a server-advertised Resume action for an OpenCode read-only root", async () => {
    const session = {
      sessionHandle: "rts_opencode_read_only",
      threadId: null,
      title: "Transferable OpenCode root",
      archived: false,
      updatedAtMs: 1_700_000_000_000,
      parentThreadId: null,
      dedupKey: "rtd_opencode_read_only",
      fidelity: "partial",
      ownership: "readOnly",
      actions: ["read", "resume", "fork"]
    };
    gatewayMock.runtimeSessionRequest = (method) => method === "runtime/session/list"
      ? {
          runtimeRef: "opencode",
          supported: true,
          sessions: [session],
          nextCursor: null
        }
      : {
          runtimeRef: "opencode",
          sessionHandle: session.sessionHandle,
          supported: true,
          changed: true,
          session: { ...session, ownership: "readWrite", actions: ["read", "fork"] },
          message: null,
          revisions: [],
          nextCursor: null
        };

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile opencode" }));
    const sessions = await within(region).findByRole("region", { name: "Native Sessions" });
    fireEvent.click(within(sessions).getByRole("button", { name: "Load sessions" }));
    expect(await within(sessions).findByText("Transferable OpenCode root")).toBeTruthy();
    fireEvent.click(within(sessions).getByRole("button", { name: "Resume" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/resume",
        params: expect.objectContaining({
          runtimeRef: "opencode",
          sessionHandle: "rts_opencode_read_only"
        })
      });
    });
  });

  it("attaches an active direct session read-only and opens its public thread", async () => {
    const activeSession = {
      sessionHandle: "rts_active_direct",
      threadId: null,
      title: "Active Codex review",
      archived: false,
      updatedAtMs: 1_700_000_000_000,
      parentThreadId: null,
      dedupKey: "rtd_active_direct",
      fidelity: "partial",
      ownership: "active",
      actions: ["read", "attach", "fork"]
    };
    gatewayMock.runtimeSessionRequest = (method) => method === "runtime/session/list"
      ? {
          runtimeRef: "codex",
          supported: true,
          sessions: [activeSession],
          nextCursor: null
        }
      : {
          runtimeRef: "codex",
          sessionHandle: activeSession.sessionHandle,
          supported: true,
          changed: true,
          session: {
            ...activeSession,
            threadId: "attached-public-thread",
            ownership: "readOnly",
            actions: ["read", "fork"]
          },
          message: "Attached read-only.",
          revisions: [],
          nextCursor: null
        };

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile codex" }));
    const sessions = await within(region).findByRole("region", { name: "Native Sessions" });
    fireEvent.click(within(sessions).getByRole("button", { name: "Load sessions" }));
    expect(await within(sessions).findByText("Active Codex review")).toBeTruthy();
    expect(within(sessions).queryByRole("button", { name: "Resume" })).toBeNull();
    fireEvent.click(within(sessions).getByRole("button", { name: "Attach read-only" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/attach",
        params: expect.objectContaining({
          runtimeRef: "codex",
          sessionHandle: "rts_active_direct"
        })
      });
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/read",
        params: { threadId: "attached-public-thread" }
      });
    });
    expect(gatewayMock.requestLog.some((entry) => (
      entry.method === "runtime/session/resume"
      || (entry.method === "thread/resume"
        && (entry.params as { threadId?: string } | undefined)?.threadId === "attached-public-thread")
    ))).toBe(false);
  });

  it("loads opaque OpenCode revision pages and invokes capability-gated revert actions", async () => {
    const session = {
      sessionHandle: "rts_opencode_session",
      threadId: null,
      title: "OpenCode review",
      archived: false,
      updatedAtMs: 1_700_000_000_000,
      parentThreadId: null,
      dedupKey: "rtd_opencode_session",
      fidelity: "partial",
      ownership: "readWrite",
      actions: ["read", "revert", "unrevert"]
    };
    const earlierSession = {
      ...session,
      sessionHandle: "rts_opencode_earlier",
      title: "Earlier OpenCode review",
      dedupKey: "rtd_opencode_earlier",
      actions: ["read"]
    };
    gatewayMock.runtimeSessionRequest = (method, params) => {
      if (method === "runtime/session/list") {
        const cursor = (params as { cursor?: string | null }).cursor ?? null;
        return {
          runtimeRef: "opencode",
          supported: true,
          sessions: cursor == null ? [session] : [earlierSession],
          nextCursor: cursor == null ? "rtl_sessions_2" : null
        };
      }
      if (method === "runtime/session/read") {
        const cursor = (params as { cursor?: string | null }).cursor ?? null;
        return {
          runtimeRef: "opencode",
          sessionHandle: session.sessionHandle,
          supported: true,
          changed: false,
          session,
          message: null,
          revisions: cursor == null
            ? [{ revisionHandle: "rtr_recent", role: "user", createdAtMs: 1_700_000_000_000 }]
            : [{ revisionHandle: "rtr_earlier", role: "user", createdAtMs: 1_600_000_000_000 }],
          nextCursor: cursor == null ? "rtc_earlier" : null
        };
      }
      return {
        runtimeRef: "opencode",
        sessionHandle: session.sessionHandle,
        supported: true,
        changed: true,
        session,
        message: null,
        revisions: [],
        nextCursor: null
      };
    };

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile opencode" }));
    const sessions = await within(region).findByRole("region", { name: "Native Sessions" });
    fireEvent.click(within(sessions).getByRole("button", { name: "Load sessions" }));
    expect(await within(sessions).findByText("OpenCode review")).toBeTruthy();
    expect(within(sessions).queryByRole("button", { name: "Revert" })).toBeNull();
    fireEvent.click(within(sessions).getByRole("button", { name: "Load more sessions" }));
    expect(await within(sessions).findByText("Earlier OpenCode review")).toBeTruthy();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "runtime/session/list",
      params: expect.objectContaining({ cursor: "rtl_sessions_2" })
    });

    fireEvent.click(within(sessions).getByRole("button", { name: "Load revisions" }));
    const picker = await within(sessions).findByRole("combobox", { name: "Revert point for OpenCode review" });
    expect(picker.getAttribute("value") ?? (picker as HTMLSelectElement).value).toBe("rtr_recent");
    expect(within(sessions).getByRole("button", { name: "Revert" })).toBeTruthy();
    fireEvent.click(within(sessions).getByRole("button", { name: "Load earlier" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/read",
        params: expect.objectContaining({ cursor: "rtc_earlier", sessionHandle: "rts_opencode_session" })
      });
    });
    fireEvent.change(picker, { target: { value: "rtr_earlier" } });
    fireEvent.click(within(sessions).getByRole("button", { name: "Revert" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/revert",
        params: expect.objectContaining({
          runtimeRef: "opencode",
          sessionHandle: "rts_opencode_session",
          revisionHandle: "rtr_earlier"
        })
      });
    });
    fireEvent.click(within(sessions).getByRole("button", { name: "Unrevert" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/unrevert",
        params: expect.objectContaining({
          sessionHandle: "rts_opencode_session",
          revisionHandle: null
        })
      });
    });
    const publicRequests = JSON.stringify(gatewayMock.requestLog);
    expect(publicRequests).not.toContain("messageID");
    expect(publicRequests).not.toContain("itemId");
  });

  it("keeps Codex revision actions unavailable and confirms archive descendant cascade", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    gatewayMock.runtimeSessionRequest = () => ({
      runtimeRef: "codex",
      supported: true,
      sessions: [{
        sessionHandle: "rts_codex",
        threadId: null,
        title: "Codex history",
        archived: false,
        updatedAtMs: null,
        parentThreadId: null,
        dedupKey: "rtd_codex",
        fidelity: "partial",
        ownership: "readWrite",
        actions: ["read", "archive", "delete", "revert", "unrevert"]
      }],
      nextCursor: null
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(region).findByRole("tab", { name: "Runtime Profiles" }));
    fireEvent.click(await within(region).findByRole("button", { name: "Runtime Profile codex" }));
    const sessions = await within(region).findByRole("region", { name: "Native Sessions" });
    fireEvent.click(within(sessions).getByRole("button", { name: "Load sessions" }));
    expect(await within(sessions).findByText("Codex history")).toBeTruthy();
    expect(within(sessions).queryByRole("button", { name: "Load revisions" })).toBeNull();
    expect(within(sessions).queryByRole("button", { name: "Revert" })).toBeNull();
    expect(within(sessions).queryByRole("button", { name: "Unrevert" })).toBeNull();
    fireEvent.click(within(sessions).getByRole("button", { name: "Archive" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/session/archive",
        params: expect.objectContaining({ runtimeRef: "codex", sessionHandle: "rts_codex" })
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      "Archive Codex history? Descendant Codex sessions will also be archived."
    );
    confirm.mockRestore();
  });

  it("keeps disabled skills visible and can re-enable them", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    const deploy = await within(region).findByRole("button", { name: "Skill deploy" });
    expect(within(deploy).getByTitle("Deploy with release checks")).toBeTruthy();
    expect(deploy.textContent).not.toContain("Disabled");
    expect(deploy.textContent).toContain("Project");
    expect(deploy.textContent).not.toContain("project");
    expect(deploy.textContent).toContain("Setup Needed");

    fireEvent.click(deploy);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/read" && JSON.stringify(entry.params).includes("deploy"))).toBe(true);
    });
    expect(within(region).getByText("DEPLOY_TOKEN")).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Enable deploy" })).toBeNull();

    const enable = within(region).getByRole("switch", { name: "Enable deploy" });
    expect(enable.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(enable);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/setEnabled" && JSON.stringify(entry.params).includes("\"enabled\":true"))).toBe(true);
    });
    await waitFor(() => {
      expect(within(region).getByRole("switch", { name: "Disable deploy" }).getAttribute("aria-checked")).toBe("true");
    });
  });

  it("uses plugin action hints, canonical selectors, and mutation scopes", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(within(region).getByRole("tab", { name: "Plugins" }));
    const browserRow = await within(region).findByRole("button", { name: "Plugin Browser" });
    fireEvent.click(browserRow);
    expect(within(region).queryByRole("button", { name: "Uninstall" })).toBeNull();
    const browserSwitch = await within(region).findByRole("switch", { name: "Disable Browser" });
    expect(browserSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(browserSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/setEnabled",
        params: expect.objectContaining({
          selector: "builtin:browser",
          scopeName: "profile",
          enabled: false
        })
      });
    });

    fireEvent.click(within(region).getByRole("button", { name: "Plugin writer-kit" }));
    const uninstall = within(region).getByRole("button", { name: "Uninstall" });
    const pluginSwitch = await within(region).findByRole("switch", { name: "Disable writer-kit" });
    expect(pluginSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(pluginSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/setEnabled",
        params: expect.objectContaining({
          selector: "profile:writer-kit@local-plugins-writer-kit",
          scopeName: "project",
          enabled: false
        })
      });
    });
    fireEvent.click(uninstall);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/uninstall",
        params: expect.objectContaining({
          selector: "profile:writer-kit@local-plugins-writer-kit",
          scopeName: "profile"
        })
      });
    });

    const duplicateRows = within(region).getAllByRole("button", { name: "Plugin dual-scope" });
    expect(duplicateRows).toHaveLength(2);
    const [profileRow, projectRow] = duplicateRows;
    if (!profileRow || !projectRow) throw new Error("expected profile and project plugin rows");
    const profileItem = profileRow.closest<HTMLElement>('[role="listitem"]');
    const projectItem = projectRow.closest<HTMLElement>('[role="listitem"]');
    if (!profileItem || !projectItem) throw new Error("expected plugin list items");
    expect(within(profileItem).getByText("Profile")).toBeTruthy();
    expect(within(projectItem).getByText("Project")).toBeTruthy();

    fireEvent.click(profileRow);
    expect(profileItem.className).toContain("is-selected");
    expect(projectItem.className).not.toContain("is-selected");
    fireEvent.click(projectRow);
    expect(projectItem.className).toContain("is-selected");
    expect(profileItem.className).not.toContain("is-selected");

    const profileSwitch = within(profileItem).getByRole("switch", { name: "Disable dual-scope" });
    await waitFor(() => expect((profileSwitch as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(profileSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/setEnabled",
        params: expect.objectContaining({
          selector: "profile:dual-scope@shared-source",
          scopeName: "profile",
          enabled: false
        })
      });
    });
    const projectSwitch = within(projectItem).getByRole("switch", { name: "Enable dual-scope" });
    await waitFor(() => expect((projectSwitch as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(projectSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/setEnabled",
        params: expect.objectContaining({
          selector: "project:dual-scope@shared-source",
          scopeName: "project",
          enabled: true
        })
      });
    });

    fireEvent.click(within(region).getByRole("tab", { name: "MCP" }));
    const mcpSwitch = await within(region).findByRole("switch", { name: "Disable docs" });
    expect(mcpSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(mcpSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "mcp/setEnabled",
        params: expect.objectContaining({
          name: "docs",
          enabled: false
        })
      });
    });
    confirm.mockRestore();
  });

  it("keeps coding-core tools read-only while web remains mode-configurable", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(within(region).getByRole("tab", { name: "Tools" }));
    const codingCore = await within(region).findByRole("button", { name: /coding-core/i });
    fireEvent.click(codingCore);

    const toolMutationsBefore = gatewayMock.requestLog.filter((entry) => entry.method === "tool/setEnabled").length;
    expect(within(region).queryByRole("button", { name: /^Default / })).toBeNull();
    expect(within(region).queryByRole("button", { name: /^Plan / })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Remove" })).toBeNull();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "tool/setEnabled").length).toBe(toolMutationsBefore);

    fireEvent.click(within(region).getByRole("button", { name: /web/i }));
    const defaultToggle = await within(region).findByRole("button", { name: "Default On" });
    expect(within(region).getByRole("button", { name: "Plan Off" })).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Remove" })).toBeNull();

    fireEvent.click(defaultToggle);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "tool/setEnabled",
        params: expect.objectContaining({
          name: "web",
          mode: "default",
          enabled: true
        })
      });
    });

    expect(within(region).queryByLabelText("Toolset name")).toBeNull();
    fireEvent.click(within(region).getByRole("button", { name: "Create toolset" }));
    fireEvent.change(await within(region).findByLabelText("Toolset name"), { target: { value: "coding-core" } });
    expect((within(region).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("searches skills and reads selected skill details", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await within(region).findByRole("button", { name: "Skill review" });
    fireEvent.change(within(region).getByLabelText("Search Skills"), { target: { value: "deploy" } });
    expect(within(region).getByRole("button", { name: "Skill deploy" })).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Skill review" })).toBeNull();

    fireEvent.click(within(region).getByRole("button", { name: "Skill deploy" }));
    await waitFor(() => {
      const read = [...gatewayMock.requestLog].reverse().find((entry) => entry.method === "skill/read");
      expect(JSON.stringify(read?.params)).toContain("/tmp/project/.psychevo/skills/deploy/SKILL.md");
    });
    const preview = within(region).getByLabelText("SKILL.md preview");
    await waitFor(() => {
      expect(within(preview).getByRole("heading", { name: "deploy workflow" })).toBeTruthy();
    });
    const table = within(preview).getByRole("table", { name: "YAML frontmatter" });
    expect(within(table).getByText("description")).toBeTruthy();
    expect(within(table).getByText("Deploy with release checks")).toBeTruthy();
    expect(within(table).getByText("Bash")).toBeTruthy();
    expect(within(preview).getByText("Follow the deploy workflow.")).toBeTruthy();
    fireEvent.click(within(preview).getByRole("button", { name: "Copy SKILL.md" }));
    await waitFor(() => {
      expect(gatewayMock.clipboardWriteLog[gatewayMock.clipboardWriteLog.length - 1]).toBe(
        "---\nname: deploy\ndescription: Deploy with release checks\nallowed-tools:\n  - Bash\n  - Read\n---\n# deploy workflow\n\nFollow the deploy workflow.\n\n- Confirm prerequisites\n\n```sh\npevo deploy\n```"
      );
    });
    fireEvent.click(within(preview).getByRole("button", { name: "Edit deploy SKILL.md" }));
    const editor = within(region).getByRole("region", { name: "SKILL.md editor" });
    const textarea = within(editor).getByRole("textbox", { name: "SKILL.md editor" });
    fireEvent.change(textarea, {
      target: {
        value: "---\nname: deploy\ndescription: Deploy with release checks\n---\n# deploy workflow\n\nUpdated deploy instructions."
      }
    });
    fireEvent.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "skill/write",
        params: expect.objectContaining({
          name: "deploy",
          path: "/tmp/project/.psychevo/skills/deploy/SKILL.md",
          target: "project",
          rawMarkdown: expect.stringContaining("Updated deploy instructions.")
        })
      });
    });
  });

  it("hides prompt and empty fields from skill details", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(await within(region).findByRole("button", { name: "Skill imagegen" }));
    const detail = within(region).getByLabelText("Skills detail");
    const preview = within(detail).getByLabelText("SKILL.md preview");
    const body = detail.querySelector(".skillDetailBody");

    await waitFor(() => {
      expect(within(preview).getByRole("heading", { name: "imagegen workflow" })).toBeTruthy();
    });
    expect(body?.children[0]?.classList.contains("skillDetailSummary")).toBe(true);
    expect(body?.children[1]).toBe(preview);
    expect(within(detail).queryByText("Prompt Visible")).toBeNull();
    expect(within(detail).queryByText("Tags")).toBeNull();
    expect(within(detail).queryByText("Missing Env")).toBeNull();
    expect(within(detail).queryByText("Missing Credentials")).toBeNull();
    expect(within(detail).queryByText("Tools")).toBeNull();
    expect(within(detail).queryByText("Toolsets")).toBeNull();
    expect(within(detail).queryByText("Linked Files")).toBeNull();
    expect(within(detail).queryByText("None")).toBeNull();
  });

  it("hides redundant skill entry file paths and shows non-standard entrypoints", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await within(region).findByText("/tmp/profile/skills/review");
    expect(within(region).queryByText("Location")).toBeNull();
    expect(within(region).queryByText("Entrypoint")).toBeNull();

    fireEvent.click(within(region).getByRole("button", { name: "Skill root-note" }));
    await waitFor(() => {
      expect(within(region).getByText("Entrypoint")).toBeTruthy();
    });
    expect(within(region).getByText("/tmp/profile/skills/root-note.md")).toBeTruthy();
    expect(within(region).getByText("/tmp/profile/skills")).toBeTruthy();
  });

  it("requires confirmation before sending a force skill install", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    expect(within(region).queryByLabelText("Skill source")).toBeNull();
    fireEvent.click(within(region).getByRole("button", { name: "Install skill" }));
    fireEvent.change(await within(region).findByLabelText("Skill source"), { target: { value: "/tmp/skills/review" } });
    fireEvent.click(within(region).getByLabelText("Force"));

    fireEvent.click(within(region).getByRole("button", { name: "Install" }));
    expect(confirm).toHaveBeenCalledWith("Install skill with force?");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/install")).toBe(false);

    fireEvent.click(within(region).getByRole("button", { name: "Install" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/install" && JSON.stringify(entry.params).includes("\"force\":true"))).toBe(true);
    });
    confirm.mockRestore();
  });

  it("inspects plugin sources before install", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(region).getByRole("tab", { name: "Plugins" }));
    await within(region).findByRole("button", { name: /writer-kit/i });

    fireEvent.click(within(region).getByRole("button", { name: "Install plugin" }));
    fireEvent.change(await within(region).findByLabelText("Plugin source"), { target: { value: "/tmp/plugins/writer-kit" } });
    fireEvent.click(within(region).getByRole("button", { name: "Inspect" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "plugin/import/inspect")).toBe(true);
    });
    expect(within(region).getByText("codex / Available")).toBeTruthy();
    expect(within(region).getByText("skills, mcp")).toBeTruthy();
    expect(within(region).getByText("apps")).toBeTruthy();
  });
});
