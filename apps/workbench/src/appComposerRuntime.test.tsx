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

describe("Workbench runtime and agent controls", () => {
  it("keeps concrete draft agent selection and submits the selected agent", async () => {
    render(<App />);

    const popover = await selectMainAgent("translate");
    const agentGroup = within(popover).getByRole("radiogroup", { name: "Main agent" });
    expect(within(agentGroup).getByRole("radio", { name: "Default Agent" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Default Permission" })).toBeTruthy();
    expect(within(agentGroup).getByRole("radio", { name: "translate" }).getAttribute("aria-checked")).toBe("true");

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({ agentName: "translate" })
      });
    });
  });

  it("submits ACP runtime plan through the shared Plan mode switch", async () => {
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

    const popover = await selectRuntime("opencode");
    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    expect(within(runtimeGroup).getByRole("radio", { name: "OpenCode" }).getAttribute("aria-checked")).toBe("true");
    const modeSelect = await screen.findByRole("combobox", { name: "OpenCode mode" }) as HTMLSelectElement;
    expect(modeSelect.value).toBe("");
    expect(within(modeSelect).getByRole("option", { name: "Default/Plan" })).toBeTruthy();
    expect(within(modeSelect).getByRole("option", { name: "Review" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Add attachments and options" }));
    fireEvent.click(screen.getByRole("switch", { name: "Plan mode" }));

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello from opencode" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: null,
          mode: null,
          runtimeRef: "opencode",
          runtimeSessionId: "opencode-session",
          runtimeOptions: { mode: "plan" }
        })
      });
    });
  });

  it("submits extra ACP runtime modes from the conditional mode selector", async () => {
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
    const modeSelect = await screen.findByRole("combobox", { name: "OpenCode mode" }) as HTMLSelectElement;
    fireEvent.change(modeSelect, { target: { value: "review" } });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "review this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: null,
          runtimeRef: "opencode",
          runtimeOptions: { mode: "review" }
        })
      });
    });
  });

  it("disables the main agent selector when the ACP runtime owns persona", async () => {
    gatewayMock.agentRecords = [agentRecord("translate", ["subagent"])];
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
        entrypoints: ["peer"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    await selectMainAgent("translate");
    const popover = await selectRuntime("opencode");
    const agentGroup = within(popover).getByRole("radiogroup", { name: "Main agent" });
    expect((within(agentGroup).getByRole("radio", { name: "Default Agent" }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(agentGroup).getByRole("radio", { name: "translate" }) as HTMLButtonElement).disabled).toBe(true);
    expect(await screen.findByText("This runtime uses its own persona.")).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "translate this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: null,
          runtimeRef: "opencode",
          runtimeOptions: { mode: "build" }
        })
      });
    });
  });

  it("omits Psychevo @agent completion candidates when a peer runtime is selected", async () => {
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
    gatewayMock.completionResult = {
      replacement: { start: 0, end: 3 },
      items: [
        {
          id: "agent:opencode",
          sigil: "@",
          label: "@opencode",
          insertText: "@opencode",
          kind: "agent",
          detail: "OpenCode ACP delegate",
          target: {
            kind: "agent",
            name: "opencode",
            source: "generated",
            entrypoints: ["subagent"],
            backendRef: "opencode"
          },
          sortText: "1:opencode"
        }
      ]
    };

    render(<App />);

    await selectRuntime("opencode");
    fireEvent.change(await screen.findByPlaceholderText("Ask Psychevo..."), { target: { value: "@op" } });

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "completion/list")).toBe(true);
    });
    expect(screen.queryByRole("option", { name: /@opencode/ })).toBeNull();
  });

  it("strips structured @agent mentions when submitting to a peer runtime", async () => {
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
    gatewayMock.completionResult = {
      replacement: { start: 0, end: 3 },
      items: [
        {
          id: "agent:opencode",
          sigil: "@",
          label: "@opencode",
          insertText: "@opencode",
          kind: "agent",
          detail: "OpenCode ACP delegate",
          target: {
            kind: "agent",
            name: "opencode",
            source: "generated",
            entrypoints: ["subagent"],
            backendRef: "opencode"
          },
          sortText: "1:opencode"
        }
      ]
    };

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "@op" } });
    const option = await screen.findByRole("option", { name: /@opencode/ });
    fireEvent.mouseDown(option);
    await waitFor(() => expect((textarea as HTMLTextAreaElement).value).toBe("@opencode "));

    await selectRuntime("opencode");
    fireEvent.change(textarea, { target: { value: "@opencode list tools" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          mentions: [],
          runtimeRef: "opencode"
        })
      });
    });
  });
});
