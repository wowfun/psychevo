// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  agentRecord,
  commandItem,
  deferred,
  gatewayMock,
  observabilityResult,
  openAgentRuntimePopover,
  selectMainAgent,
  selectRuntime,
  sessionSummary,
  workspaceDiffAction
} from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench settings and backend controls", () => {
  it("unmounts hidden left sidebar sections when collapsed", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Collapse left sidebar" }));

    expect(screen.getByRole("button", { name: "New Session" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Search" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Artifacts" })).toBeNull();
    expect(screen.queryByText("Pinned")).toBeNull();
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
  });

  it("shows an explicit Settings return action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    expect(settingsRegion).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Appearance" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("heading", { name: "Appearance" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Archived sessions" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Usage" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Debug" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Agents" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Dark" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Light" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Warm" })).toBeTruthy();
    for (const removed of ["General", "Session", "Session history", "Commands", "Integrations", "Diagnostics"]) {
      expect(within(settingsRegion).queryByRole("button", { name: removed })).toBeNull();
    }
    expect(within(settingsRegion).getByRole("searchbox", { name: "Search settings" })).toBeTruthy();
    expect(within(settingsRegion).queryByText("/tmp/project")).toBeNull();
    expect(within(settingsRegion).queryByRole("heading", { name: "Settings" })).toBeNull();
    expect(within(settingsRegion).queryByRole("button", { name: "Back to transcript" })).toBeNull();

    const backButton = within(settingsRegion).getByRole("button", { name: "Back to app" });

    fireEvent.click(backButton);
    expect(await screen.findByRole("region", { name: "Transcript" })).toBeTruthy();
  });

  it("loads all-history usage summaries in Settings", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Usage" }));

    const usagePanel = await within(settingsRegion).findByRole("region", { name: "Usage" });
    expect(await within(usagePanel).findByText("All time")).toBeTruthy();
    expect(within(usagePanel).getByText("Last 30 days")).toBeTruthy();
    expect(within(usagePanel).getByText("Last 7 days")).toBeTruthy();
    expect(within(usagePanel).getByText("Token activity")).toBeTruthy();
    expect(within(usagePanel).getByRole("button", { name: "Refresh usage" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => (
      entry.method === "usage/read"
      && (entry.params as { activityDays?: number }).activityDays === 365
    ))).toBe(true);
  });

  it("switches Settings sections while keeping session controls in the composer", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    expect(within(settingsRegion).getByRole("button", { name: "Agents" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("region", { name: "Agents" })).toBeTruthy();
    expect(within(settingsRegion).getByText("Profile ACP Backends")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Translate user messages")).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Agent" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Model" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Permission mode" })).toBeNull();

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    expect(await screen.findByRole("button", { name: "Agent" })).toBeTruthy();
  });

  it("shows archived sessions from Settings without turning the sidebar into an archive filter", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("active-thread", "Active session")];
    gatewayMock.archivedSessionSummaries = [sessionSummary("archived-thread", "Archived session")];

    render(<App />);

    expect(await screen.findByText("Active session")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Archived sessions" }));

    const archivedPanel = await within(settingsRegion).findByRole("region", { name: "Archived sessions" });
    expect(await within(archivedPanel).findByText("Archived session")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Active session")).toBeNull();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/list",
      params: expect.objectContaining({ archived: true })
    });
  });

  it("renders provider-qualified model options in the selected indicator", async () => {
    render(<App />);

    const modelSelect = await screen.findByRole("combobox", { name: "Model" }) as HTMLSelectElement;
    expect(modelSelect.selectedOptions[0]?.textContent).toBe("xiaomi/xiaomi-token-high");
    expect(modelSelect.title).toBe("xiaomi/xiaomi-token-high");
    expect(screen.getByRole("option", { name: "openai/gpt-4o" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "xiaomi/xiaomi-token-high" })).toBeTruthy();
    expect(screen.getAllByText("xiaomi/xiaomi-token-high").length).toBeGreaterThan(0);
    expect(screen.queryByText("xiaomi-token-high")).toBeNull();
    expect(modelSelect.closest(".statusSelect")?.getAttribute("style")).toContain("--pevo-status-select-value-width: 25ch");
    const variantSelect = screen.getByRole("combobox", { name: "Variant" }) as HTMLSelectElement;
    expect(variantSelect.selectedOptions[0]?.textContent).toBe("default");
    expect(screen.queryByText("medium")).toBeNull();
  });

  it("blocks prompt turns until a concrete provider-qualified model is selected", async () => {
    gatewayMock.model = null;
    gatewayMock.modelStatus = "unconfigured";

    render(<App />);

    expect((await screen.findAllByText("Select model")).length).toBeGreaterThan(0);
    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello" } });
    const sendButton = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    expect(sendButton.disabled).toBe(true);
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    fireEvent.change(screen.getByRole("combobox", { name: "Model" }), { target: { value: "openai/gpt-4o" } });
    expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "hello" }],
          model: "openai/gpt-4o"
        })
      });
    });
  });

  it("keeps ACP peer backends in Runtime instead of the composer Agent selector", async () => {
    gatewayMock.agentRecords = [
      agentRecord("opencode", ["subagent"], "opencode"),
      agentRecord("cursor", ["peer", "subagent"], "cursor"),
      agentRecord("translate", ["subagent"])
    ];
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      },
      {
        id: "cursor",
        kind: "acp",
        enabled: true,
        label: "Cursor",
        description: null,
        command: "cursor-agent",
        args: ["--acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    const popover = await openAgentRuntimePopover();
    const agentGroup = within(popover).getByRole("radiogroup", { name: "Main agent" });
    expect(within(agentGroup).getByRole("radio", { name: "Default Agent" })).toBeTruthy();
    expect(within(agentGroup).queryByRole("radio", { name: "cursor" })).toBeNull();
    expect(within(agentGroup).queryByRole("radio", { name: "opencode" })).toBeNull();

    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    expect(within(runtimeGroup).getByRole("radio", { name: "Native Runtime" })).toBeTruthy();
    expect(within(runtimeGroup).getByRole("radio", { name: "Cursor" })).toBeTruthy();
    expect(within(runtimeGroup).queryByRole("radio", { name: "OpenCode" })).toBeNull();
  });

  it("clears a selected ACP runtime when its peer entrypoint is disabled", async () => {
    gatewayMock.agentRecords = [agentRecord("opencode", ["peer", "subagent"], "opencode")];
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    await selectRuntime("opencode");
    const popover = await openAgentRuntimePopover();
    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    await waitFor(() => expect(within(runtimeGroup).getByRole("radio", { name: "OpenCode" }).getAttribute("aria-checked")).toBe("true"));

    gatewayMock.agentRecords = [agentRecord("opencode", ["subagent"], "opencode")];
    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));
    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });
    fireEvent.click(await within(agentsPanel).findByLabelText("opencode peer entrypoint"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          entrypoints: ["subagent"]
        })
      });
    });

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    const nextPopover = await openAgentRuntimePopover();
    const nextRuntimeGroup = within(nextPopover).getByRole("radiogroup", { name: "Runtime" });
    await waitFor(() => expect(within(nextRuntimeGroup).getByRole("radio", { name: "Native Runtime" }).getAttribute("aria-checked")).toBe("true"));
    expect(within(nextRuntimeGroup).queryByRole("radio", { name: "OpenCode" })).toBeNull();
  });

  it("creates a Profile ACP backend from the generic Settings Agents add action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });
    const addButton = within(agentsPanel).getByRole("button", { name: "Add ACP backend" });
    expect(addButton.textContent).toBe("");
    fireEvent.click(addButton);
    const form = await within(agentsPanel).findByRole("form", { name: "Profile ACP backend" });
    expect(within(form).queryByLabelText("Target")).toBeNull();
    expect((within(form).getByLabelText("ID") as HTMLInputElement).value).toBe("");
    const commandJson = within(form).getByLabelText("Command JSON") as HTMLTextAreaElement;
    expect(JSON.parse(commandJson.value)).toEqual({
      command: "opencode",
      args: ["acp"],
      env: {}
    });
    expect(commandJson.placeholder).toBe("");
    expect(within(form).queryByLabelText("Command")).toBeNull();
    expect(within(form).queryByLabelText("Args")).toBeNull();
    expect(within(form).queryByLabelText("Env")).toBeNull();
    const cwd = within(form).getByLabelText("CWD") as HTMLInputElement;
    expect(cwd.value).toBe("");
    expect(cwd.placeholder).toBe("Defaults to workspace");
    expect(within(form).getByLabelText("Label").closest("label")?.textContent).toContain("Optional");
    expect(within(form).getByLabelText("Description").closest("label")?.textContent).toContain("Optional");
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    expect(within(form).queryByLabelText("Enabled")).toBeNull();
    expect(within(form).queryByText("Entrypoints")).toBeNull();
    fireEvent.change(cwd, { target: { value: "agents" } });
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    fireEvent.change(cwd, { target: { value: "/opt/acp" } });
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    fireEvent.change(cwd, { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("ID"), { target: { value: "local-acp" } });
    expect((within(form).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.change(commandJson, { target: { value: "{" } });
    expect(within(form).getByText("Command JSON must be valid JSON.")).toBeTruthy();
    expect((within(form).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(commandJson, {
      target: {
        value: JSON.stringify({
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" }
        }, null, 2)
      }
    });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "local-acp",
          target: "profile",
          label: null,
          description: null,
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" },
          cwd: "invocation",
          entrypoints: ["peer", "subagent"],
          clientCapabilities: ["fs.read", "fs.write", "terminal"]
        })
      });
    });
  });

  it("updates Profile ACP backend enabled and entrypoints from Settings Agents rows", async () => {
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));
    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });

    fireEvent.click(await within(agentsPanel).findByRole("switch", { name: "Disable opencode" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["peer", "subagent"]
        })
      });
    });

    fireEvent.click(await within(agentsPanel).findByLabelText("opencode peer entrypoint"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["subagent"]
        })
      });
    });
  });
});
