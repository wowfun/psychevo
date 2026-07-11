// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  agentRecord,
  commandItem,
  deferred,
  gatewayMock,
  observabilityResult,
  openAgentRuntimePopover,
  openRuntimeProfilePopover,
  selectMainAgent,
  selectRuntime,
  sessionSummary,
  workspaceDiffAction
} from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench runtime and agent controls", () => {
  it("projects the native mode only through the shared Plan affordance", async () => {
    render(<App />);

    await screen.findByRole("button", { name: "Runtime Profile" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/context/read")).toBe(true);
    });
    expect(screen.queryByRole("combobox", { name: "Mode" })).toBeNull();
    expect(screen.queryByLabelText("Runtime control state")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add attachments and options" }));
    fireEvent.click(screen.getByRole("switch", { name: "Plan mode" }));
    expect(screen.getByText("Plan")).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "plan this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          mode: "plan",
          runtimeOptions: {}
        })
      });
    });
  });

  it("omits the branch placeholder when the workspace has no git branch", async () => {
    gatewayMock.projectBranch = null;

    render(<App />);

    await screen.findByText("/tmp/project");
    await waitFor(() => {
      expect(screen.queryByText("no-branch")).toBeNull();
    });
  });

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

  it("shows composer feedback when a permission response is rejected", async () => {
    gatewayMock.snapshot.pendingActions = [
      {
        actionId: "permission-1",
        kind: "permission",
        title: "exec_command",
        summary: "Run exec_command",
        payload: {
          toolName: "exec_command",
          summary: "Run exec_command",
          reason: "requires approval",
          matchedRule: "exec:python3 -c",
          suggestedRule: null,
          allowAlways: false,
          timeoutSecs: 300
        },
        threadId: "thread-1",
        turnId: "turn-1",
        activityId: "activity-1",
        sourceKey: "source-thread-1"
      }
    ];
    gatewayMock.permissionRespond = () => ({ accepted: false });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Once" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "permission/respond",
        params: {
          requestId: "permission-1",
          threadId: "thread-1",
          sourceKey: "source-thread-1",
          activityId: "activity-1",
          decision: "allowOnce"
        }
      });
    });
    expect(await screen.findByText("Permission response was not accepted.")).toBeTruthy();
  });

  it("shows shared Attention provenance and only adapter-enforced authorization lifetimes", async () => {
    gatewayMock.snapshot.pendingActions = [
      {
        actionId: "runtime-permission-1",
        kind: "permission",
        title: "command",
        summary: "Run the requested command",
        payload: {
          runtimeRef: "codex-review",
          runtimeKind: "codex",
          profileLabel: "Codex Review",
          toolName: "command",
          summary: "Run the requested command",
          reason: "The command needs approval",
          allowSession: true,
          allowAlways: false,
          authorizationLifetime: "codex_session",
          origin: {
            parentThreadId: "parent-public-thread",
            childThreadId: "child-public-thread"
          }
        },
        threadId: "parent-public-thread",
        turnId: "turn-1"
      }
    ];

    render(<App />);

    expect(await screen.findByText("Codex · Codex Review (codex-review)")).toBeTruthy();
    expect(screen.getByText("Child child-public-thread · Parent parent-public-thread")).toBeTruthy();
    expect(screen.getByText("Once · this request only")).toBeTruthy();
    expect(screen.getByText("Session · current Codex session")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Session" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Always" })).toBeNull();
  });

  it("omits Session and Always when Attention does not declare enforceable lifetimes", async () => {
    gatewayMock.snapshot.pendingActions = [
      {
        actionId: "runtime-permission-once",
        kind: "permission",
        title: "command",
        summary: "Run once",
        payload: {
          runtimeRef: "opencode",
          runtimeKind: "opencode",
          profileLabel: "OpenCode",
          toolName: "command",
          summary: "Run once",
          allowSession: false,
          allowAlways: false,
          origin: { parentThreadId: "parent-public-thread" }
        },
        threadId: "parent-public-thread",
        turnId: "turn-1"
      }
    ];

    render(<App />);

    await screen.findByText("OpenCode · OpenCode (opencode)");
    expect(screen.queryByRole("button", { name: "Session" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Always" })).toBeNull();
  });

  it("names OpenCode instance-scoped authorization without presenting it as permanent", async () => {
    gatewayMock.snapshot.pendingActions = [
      {
        actionId: "opencode-permission",
        kind: "permission",
        title: "permission",
        summary: "Use the requested path",
        payload: {
          runtimeRef: "opencode",
          runtimeKind: "opencode",
          profileLabel: "OpenCode",
          allowSession: true,
          allowAlways: false,
          authorizationLifetime: "until_runtime_instance_restarts",
          origin: { parentThreadId: "parent-public-thread" }
        },
        threadId: "parent-public-thread",
        turnId: "turn-1"
      }
    ];

    render(<App />);

    expect(await screen.findByText("Session · until the runtime instance restarts")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Session" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Always" })).toBeNull();
  });

  it("shows live draft permission requests and routes allow-always by request context", async () => {
    render(<App />);

    await screen.findByPlaceholderText("Ask Psychevo...");
    gatewayMock.subscribers.forEach((subscriber) => subscriber({
      method: "gateway/event",
      params: {
        type: "actionRequested",
        action: {
          actionId: "permission-draft",
          kind: "permission",
          title: "exec_command",
          summary: "inline Python could not be statically reduced",
          payload: {
            toolName: "exec_command",
            summary: "inline Python could not be statically reduced",
            reason: "requires approval",
            matchedRule: "exec:python3 -c",
            suggestedRule: "exec:python3 -c",
            allowAlways: true,
            alwaysAuthorizationLifetime: "permanent",
            timeoutSecs: 300
          },
          turnId: "turn-draft",
          activityId: "activity-draft",
          sourceKey: "web:draft"
        }
      }
    }));

    expect(await screen.findByText("inline Python could not be statically reduced")).toBeTruthy();
    expect(screen.getAllByText("exec:python3 -c").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Always" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "permission/respond",
        params: {
          requestId: "permission-draft",
          threadId: null,
          sourceKey: "web:draft",
          activityId: "activity-draft",
          decision: "allowAlways"
        }
      });
    });
  });

  it("submits structured live clarify answers and supports cancel", async () => {
    render(<App />);

    await screen.findByPlaceholderText("Ask Psychevo...");
    gatewayMock.subscribers.forEach((subscriber) => subscriber({
      method: "gateway/event",
      params: {
        type: "actionRequested",
        action: {
          actionId: "clarify-draft",
          kind: "clarify",
          payload: {
            runtimeRef: "opencode-build",
            runtimeKind: "opencode",
            profileLabel: "OpenCode Build",
            origin: {
              parentThreadId: "parent-public-thread",
              childThreadId: "child-public-thread"
            },
            raw: {
              questions: [
                {
                  question: "Which environment should I use?",
                  multiple: false,
                  custom: true,
                  options: [
                    { label: "Local", description: "Use local files" },
                    { label: "Remote", description: "Use remote API" }
                  ]
                },
                {
                  question: "How should I proceed?",
                  multiple: true,
                  custom: false,
                  options: [
                    { label: "Fix", description: "Apply the patch" },
                    { label: "Explain", description: "Only explain" }
                  ]
                },
                {
                  question: "Which verification depth?",
                  multiple: false,
                  custom: false,
                  options: [{ label: "Focused", description: "Run focused tests" }]
                },
                {
                  question: "Which report format?",
                  multiple: false,
                  custom: false,
                  options: [{ label: "Concise", description: "Keep the report concise" }]
                }
              ]
            }
          },
          turnId: "turn-draft",
          activityId: "activity-draft",
          sourceKey: "web:draft"
        }
      }
    }));

    expect(await screen.findByText("Which environment should I use?")).toBeTruthy();
    expect(screen.getByText("OpenCode · OpenCode Build (opencode-build)")).toBeTruthy();
    expect(screen.getByText("Child child-public-thread · Parent parent-public-thread")).toBeTruthy();
    const environmentQuestion = screen.getByRole("group", { name: "Which environment should I use?" });
    fireEvent.click(within(environmentQuestion).getByRole("radio", { name: /Other/ }));
    fireEvent.change(await within(environmentQuestion).findByRole("textbox"), { target: { value: "Use a temporary sandbox" } });
    const approachQuestion = screen.getByRole("group", { name: "How should I proceed?" });
    expect(within(approachQuestion).queryByRole("checkbox", { name: /Other/ })).toBeNull();
    fireEvent.click(within(approachQuestion).getByRole("checkbox", { name: /Fix/ }));
    fireEvent.click(within(approachQuestion).getByRole("checkbox", { name: /Explain/ }));
    fireEvent.click(within(screen.getByRole("group", { name: "Which verification depth?" })).getByRole("radio", { name: /Focused/ }));
    fireEvent.click(within(screen.getByRole("group", { name: "Which report format?" })).getByRole("radio", { name: /Concise/ }));
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "clarify/respond",
        params: {
          requestId: "clarify-draft",
          threadId: null,
          sourceKey: "web:draft",
          activityId: "activity-draft",
          answers: [["Use a temporary sandbox"], ["Fix", "Explain"], ["Focused"], ["Concise"]],
          cancel: false
        }
      });
    });

    gatewayMock.subscribers.forEach((subscriber) => subscriber({
      method: "gateway/event",
      params: {
        type: "actionResolved",
        actionId: "clarify-draft",
        kind: "clarify",
        outcome: "accepted",
        payload: { reason: "answered" }
      }
    }));
    await waitFor(() => {
      expect(screen.queryByText("Which environment should I use?")).toBeNull();
    });

    gatewayMock.requestLog.length = 0;
    gatewayMock.subscribers.forEach((subscriber) => subscriber({
      method: "gateway/event",
      params: {
        type: "actionRequested",
        action: {
          actionId: "clarify-cancel",
          kind: "clarify",
          payload: {
            raw: {
              questions: [
                {
                  question: "Cancel this request?",
                  options: [
                    { label: "Yes", description: "" },
                    { label: "No", description: "" }
                  ]
                }
              ]
            }
          },
          activityId: "activity-cancel",
          sourceKey: "web:draft"
        }
      }
    }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "clarify/respond",
        params: {
          requestId: "clarify-cancel",
          threadId: null,
          sourceKey: "web:draft",
          activityId: "activity-cancel",
          answers: null,
          cancel: true
        }
      });
    });
  });

  it("reads Runtime Profile choices and selectable controls from Runtime Context", async () => {
    gatewayMock.backendRecords = [{
      id: "cursor",
      kind: "acp",
      enabled: true,
      label: "Cursor",
      description: null,
      command: "cursor",
      args: ["acp"],
      cwd: "invocation",
      entrypoints: ["peer"],
      clientCapabilities: [],
      mcpServers: [],
      envKeys: [],
      sourceTargets: ["profile"],
      diagnostics: []
    }];
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "opencode",
      selectionState: "draft",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: null,
      controls: [
        {
          id: "mode",
          label: "OpenCode mode",
          state: "selectable",
          currentValue: "build",
          choices: [
            { value: "build", label: "Build", description: null },
            { value: "plan", label: "Plan", description: null },
            { value: "review", label: "Review", description: null }
          ],
          channelSafe: true,
          capabilityRevision: "7"
        },
        {
          id: "effort",
          label: "Effort",
          state: "selectable",
          currentValue: "medium",
          choices: [
            { value: "medium", label: "Medium", description: null },
            { value: "high", label: "High", description: null }
          ],
          channelSafe: false,
          capabilityRevision: "7"
        }
      ],
      activeSession: null
    });

    render(<App />);

    await waitFor(() => expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/context/read")).toBe(true));
    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "runtime/context/read",
      params: expect.objectContaining({ runtimeRef: "opencode" })
    }));
    const runtimePopover = await openRuntimeProfilePopover();
    const runtimeGroup = within(runtimePopover).getByRole("radiogroup", { name: "Runtime" });
    expect(within(runtimeGroup).getByRole("radio", { name: "OpenCode" }).getAttribute("aria-checked")).toBe("true");
    expect(within(runtimeGroup).queryByRole("radio", { name: "Cursor (ACP)" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Runtime Profile" }));

    const modeSelect = await screen.findByRole("combobox", { name: "OpenCode mode" }) as HTMLSelectElement;
    expect(screen.queryByRole("button", { name: "Model" })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Permission mode" })).toBeNull();
    expect(screen.getByLabelText("Runtime Profile safety policy").textContent).toContain("Profile safety");
    fireEvent.click(screen.getByRole("button", { name: "Add attachments and options" }));
    expect(screen.queryByRole("switch", { name: "Plan mode" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add attachments and options" }));
    expect(within(modeSelect).getByRole("option", { name: "Build" })).toBeTruthy();
    expect(within(modeSelect).getByRole("option", { name: "Review" })).toBeTruthy();
    fireEvent.change(modeSelect, { target: { value: "2" } });
    const effortSelect = screen.getByRole("combobox", { name: "Effort" }) as HTMLSelectElement;
    fireEvent.change(effortSelect, { target: { value: "1" } });

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "review this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: null,
          mode: null,
          runtimeRef: "opencode",
          runtimeSessionId: null,
          runtimeOptions: { effort: "high", mode: "review" }
        })
      });
    });
    const directTurn = gatewayMock.requestLog.find((entry) => entry.method === "turn/start");
    expect(directTurn?.params).toEqual(expect.objectContaining({
      model: null,
      permissionMode: null,
      reasoningEffort: null
    }));
    expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/options")).toBe(false);
  });

  it("submits capability revisions above Number.MAX_SAFE_INTEGER as decimal strings", async () => {
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "bound",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "runtime",
        nativeKind: "codex",
        sessionHandle: "rts_codex",
        cwd: "/tmp/project",
        profileFingerprint: "fingerprint",
        ownership: "readWrite",
        bindingRevision: 9
      },
      controls: [{
        id: "mode",
        label: "Mode",
        state: "selectable",
        currentValue: "review",
        choices: [
          { value: "review", label: "Review", description: null },
          { value: "plan", label: "Plan", description: null }
        ],
        channelSafe: true,
        capabilityRevision: "9007199254740993"
      }],
      activeSession: null,
      children: []
    });

    render(<App />);
    const mode = await screen.findByRole("combobox", { name: "Mode" });
    fireEvent.change(mode, { target: { value: "1" } });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/control/set",
        params: expect.objectContaining({
          runtimeRef: "codex",
          controlId: "mode",
          expectedCapabilityRevision: "9007199254740993",
          expectedBindingRevision: 9
        })
      });
    });
  });

  it("normalizes ACP compatibility Profile labels without reading backend/list", async () => {
    gatewayMock.runtimeProfileRecords = [...gatewayMock.runtimeProfileRecords, {
      id: "acp:cursor",
      runtime: "acp",
      enabled: true,
      label: "Cursor (ACP)",
      generated: true,
      configured: false,
      command: "cursor",
      args: ["acp"],
      backendRef: "cursor",
      provenance: "ACP",
      profileRevision: "2",
      capabilityRevision: "2",
      defaultModel: null,
      defaultMode: null,
      defaultAgent: null,
      approvalMode: null,
      sandbox: null,
      workspaceRoots: [],
      envKeys: [],
      optionKeys: [],
      sourceTargets: [],
      health: { status: "unchecked", summary: "Not checked", commandPath: null, checkedAtMs: null },
      readinessStages: [],
      diagnostics: []
    }];

    render(<App />);

    const popover = await openRuntimeProfilePopover();
    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    expect(within(runtimeGroup).getByRole("radio", { name: "Cursor (ACP)" })).toBeTruthy();
    expect(within(runtimeGroup).queryByText("Cursor (ACP) (ACP)")).toBeNull();
  });

  it("keeps the native composer model outside ACP and serializes an observed ACP model control", async () => {
    const acpProfile = {
      id: "acp:visual",
      runtime: "acp",
      enabled: true,
      label: "Visual ACP (ACP)",
      generated: true,
      configured: false,
      command: "visual-acp",
      args: [],
      backendRef: "visual-acp",
      provenance: "ACP",
      profileRevision: "4",
      capabilityRevision: "4",
      defaultModel: null,
      defaultMode: null,
      defaultAgent: null,
      approvalMode: null,
      sandbox: null,
      workspaceRoots: [],
      envKeys: [],
      optionKeys: ["model"],
      sourceTargets: [],
      health: { status: "unchecked", summary: "Not checked", commandPath: null, checkedAtMs: null },
      readinessStages: [],
      diagnostics: []
    };
    gatewayMock.runtimeProfileRecords = [...gatewayMock.runtimeProfileRecords, acpProfile];
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "acp:visual",
      selectionState: "draft",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: null,
      controls: [{
        id: "model",
        label: "Model",
        state: "selectable",
        currentValue: null,
        choices: [{ value: "lmstudio/noop", label: "Noop", description: null }],
        dependsOn: null,
        channelSafe: true,
        capabilityRevision: "4"
      }],
      capabilities: [],
      activeSession: null
    });

    render(<App />);

    const modelControl = await screen.findByRole("combobox", { name: "Model" });
    fireEvent.change(modelControl, { target: { value: "0" } });
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "use the ACP model" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          runtimeRef: "acp:visual",
          model: null,
          reasoningEffort: null,
          runtimeOptions: { model: "lmstudio/noop" }
        })
      });
    });
  });

  it("renders the Runtime default control state when no metadata is available", async () => {
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "opencode",
      selectionState: "draft",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: null,
      controls: [],
      activeSession: null
    });

    render(<App />);

    expect((await screen.findByLabelText("Runtime control state")).textContent).toContain("Runtime default");
  });

  it("does not claim the first selectable choice when the runtime has no observed value", async () => {
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "draft",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: null,
      controls: [{
        id: "model",
        label: "Model",
        state: "selectable",
        currentValue: null,
        choices: [
          { value: "gpt-first", label: "GPT First", description: null },
          { value: "gpt-second", label: "GPT Second", description: null }
        ],
        channelSafe: false,
        capabilityRevision: "9"
      }],
      capabilities: [],
      activeSession: null
    });

    render(<App />);

    const model = await screen.findByRole("combobox", { name: "Model" }) as HTMLSelectElement;
    expect(model.selectedOptions[0]?.textContent).toBe("Runtime default");
    fireEvent.change(model, { target: { value: "0" } });
    expect(model.selectedOptions[0]?.textContent).toBe("GPT First");
    fireEvent.change(model, { target: { value: "__runtime_default__" } });
    expect(model.selectedOptions[0]?.textContent).toBe("Runtime default");

    const composer = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "use the runtime default" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      const turn = gatewayMock.requestLog.find((entry) => entry.method === "turn/start");
      expect(turn?.params).toEqual(expect.objectContaining({
        runtimeOptions: expect.not.objectContaining({ model: expect.anything() })
      }));
    });
  });

  it("hides and clears model-dependent controls when the selected model changes", async () => {
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "draft",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: null,
      controls: [
        {
          id: "model",
          label: "Model",
          state: "selectable",
          currentValue: null,
          choices: [
            { value: "model-a", label: "Model A", description: null },
            { value: "model-b", label: "Model B", description: null }
          ],
          dependsOn: null,
          channelSafe: false,
          capabilityRevision: "9"
        },
        {
          id: "effort",
          label: "Reasoning effort",
          state: "selectable",
          currentValue: null,
          choices: [{ value: "high", label: "High", description: null }],
          dependsOn: { controlId: "model", value: "model-a" },
          channelSafe: false,
          capabilityRevision: "9"
        }
      ],
      capabilities: [],
      activeSession: null
    });

    render(<App />);

    const model = await screen.findByRole("combobox", { name: "Model" }) as HTMLSelectElement;
    const effort = screen.getByRole("combobox", { name: "Reasoning effort" }) as HTMLSelectElement;
    fireEvent.change(effort, { target: { value: "0" } });
    fireEvent.change(model, { target: { value: "1" } });
    expect(screen.queryByRole("combobox", { name: "Reasoning effort" })).toBeNull();

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "use model b" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      const turn = gatewayMock.requestLog.find((entry) => entry.method === "turn/start");
      expect(turn?.params).toEqual(expect.objectContaining({
        runtimeOptions: { model: "model-b" }
      }));
    });
  });

  it("keeps a bound Runtime Profile immutable and switches through a new thread", async () => {
    gatewayMock.runtimeContextRead = () => ({
        runtimeRef: "codex",
        selectionState: "bound",
        profiles: gatewayMock.runtimeProfileRecords,
        binding: {
          threadId: "thread-1",
          runtimeRef: "codex",
          backendKind: "runtime",
          nativeKind: "codex",
          sessionHandle: "codex-session-1",
          cwd: "/tmp/project",
          profileFingerprint: "codex-fingerprint",
          ownership: "readWrite",
          bindingRevision: 9
        },
        controls: [{
          id: "mode",
          label: "Mode",
          state: "readOnlyCurrent",
          currentValue: "auto-review",
          choices: [],
          channelSafe: true,
          capabilityRevision: "8"
        }],
        activeSession: {
          sessionHandle: "codex-session-1",
          threadId: "thread-1",
          title: "Bound Codex session",
          archived: false,
          updatedAtMs: 100,
          parentThreadId: null,
          dedupKey: "codex-session-1",
          fidelity: "partial",
          ownership: "readWrite",
          actions: ["fork"]
        }
      });

    render(<App />);

    const capsule = await screen.findByRole("button", { name: "Bound Runtime Profile Codex · Direct" });
    expect(capsule.className).toContain("runtimeProvenanceCapsule");
    expect(screen.getByLabelText("Mode: auto-review (read-only)")).toBeTruthy();
    fireEvent.click(capsule);
    const popover = await screen.findByRole("dialog", { name: "Runtime Profile selection" });
    fireEvent.click(within(popover).getByRole("radio", { name: "Start a new thread with OpenCode" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/start")).toBe(true);
      expect(screen.getByRole("button", { name: "Runtime Profile" }).textContent).toContain("OpenCode");
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "runtime/control/set")).toBe(false);
  });

  it("waits for the exact active turn before exposing direct runtime steer", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Active Codex session")];
    gatewayMock.snapshot.activity = { running: true, activeTurnId: null, queuedTurns: 0 };
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "bound",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "runtime",
        nativeKind: "codex",
        sessionHandle: "codex-session-1",
        cwd: "/tmp/project",
        profileFingerprint: "codex-fingerprint",
        ownership: "readWrite",
        bindingRevision: 9
      },
      controls: [],
      capabilities: [{ id: "turn.steer", enabled: true, stability: "stable" }],
      activeSession: null
    });

    render(<App />);

    fireEvent.click(await screen.findByText("Active Codex session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    await screen.findByRole("button", { name: "Bound Runtime Profile Codex · Direct" });
    const composer = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "steer only after start" } });
    expect(screen.queryByRole("button", { name: "Steer" })).toBeNull();
    await waitFor(() => expect(gatewayMock.subscribers.length).toBeGreaterThan(0));

    await act(async () => {
      gatewayMock.subscribers.forEach((subscriber) => subscriber({
        method: "gateway/event",
        params: {
          type: "turnStarted",
          threadId: "thread-1",
          turnId: "turn-direct-1",
          selectedSkills: []
        }
      }));
    });
    expect(await screen.findByRole("button", { name: "Steer" })).toBeTruthy();
    fireEvent.keyDown(composer, { key: "Enter" });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/steer",
        params: {
          expectedTurnId: "turn-direct-1",
          threadId: "thread-1",
          text: "steer only after start"
        }
      });
    });
  });

  it("does not revive Steer from a completed turn while its follow-up is queued", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Queued Codex session")];
    gatewayMock.snapshot.activity = { running: true, activeTurnId: "turn-finished", queuedTurns: 0 };
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "bound",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "runtime",
        nativeKind: "codex",
        sessionHandle: "codex-session-1",
        cwd: "/tmp/project",
        profileFingerprint: "codex-fingerprint",
        ownership: "readWrite",
        bindingRevision: 9
      },
      controls: [],
      capabilities: [{ id: "turn.steer", enabled: true, stability: "stable" }],
      activeSession: null
    });

    render(<App />);
    fireEvent.click(await screen.findByText("Queued Codex session"));
    await waitFor(() => expect(gatewayMock.subscribers.length).toBeGreaterThan(0));

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "thread-1",
            turnId: "turn-finished",
            turn: {
              id: "turn-finished",
              threadId: "thread-1",
              status: "completed",
              outcome: "normal",
              error: null,
              startedAtMs: 1,
              completedAtMs: 2
            },
            committedEntries: []
          }
        });
        subscriber({
          method: "gateway/event",
          params: {
            type: "activityChanged",
            threadId: "thread-1",
            activity: { running: true, activeTurnId: "turn-finished", queuedTurns: 1 }
          }
        });
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnQueued",
            threadId: "thread-1",
            turnId: "turn-follow-up",
            queuePosition: 1
          }
        });
      }
    });

    const composer = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "steer only after queued turn starts" } });
    expect(screen.queryByRole("button", { name: "Steer" })).toBeNull();

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnStarted",
            threadId: "thread-1",
            turnId: "turn-follow-up",
            selectedSkills: []
          }
        });
      }
    });
    expect(await screen.findByRole("button", { name: "Steer" })).toBeTruthy();
    fireEvent.keyDown(composer, { key: "Enter" });
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/steer",
        params: {
          expectedTurnId: "turn-follow-up",
          threadId: "thread-1",
          text: "steer only after queued turn starts"
        }
      });
    });
  });

  it("changes Agent Definition on a bound direct runtime through a new thread", async () => {
    gatewayMock.agentRecords = [agentRecord("translate", ["subagent"])];
    gatewayMock.runtimeContextRead = () => ({
      runtimeRef: "codex",
      selectionState: "bound",
      profiles: gatewayMock.runtimeProfileRecords,
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "runtime",
        nativeKind: "codex",
        sessionHandle: "codex-session-1",
        cwd: "/tmp/project",
        profileFingerprint: "codex-fingerprint",
        ownership: "readWrite",
        bindingRevision: 9
      },
      controls: [],
      activeSession: null
    });

    render(<App />);
    await screen.findByRole("button", { name: "Bound Runtime Profile Codex · Direct" });
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "bind the direct thread" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({ runtimeRef: "codex", threadId: null })
      });
    });
    const threadStartsBefore = gatewayMock.requestLog.filter((entry) => entry.method === "thread/start").length;
    const agentPopover = await openAgentRuntimePopover();

    fireEvent.click(within(agentPopover).getByRole("radio", {
      name: "Start a new thread with translate"
    }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/start")).toHaveLength(threadStartsBefore + 1);
    });
    expect(gatewayMock.requestLog.some((entry) => (
      entry.method === "settings/update"
      && (entry.params as { agent?: string | null } | undefined)?.agent === "translate"
    ))).toBe(false);

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "translate in a fresh thread" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: "translate",
          runtimeRef: "codex",
          threadId: null
        })
      });
    });
  });

  it("restores runtime-child fidelity from persisted context and refreshes it during lazy history read", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Parent runtime session")];
    gatewayMock.runtimeContextRead = (params) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      if (threadId === "runtime-child-thread") {
        return {
          runtimeRef: "opencode",
          selectionState: "bound",
          profiles: gatewayMock.runtimeProfileRecords,
          binding: {
            threadId,
            runtimeRef: "opencode",
            backendKind: "runtime",
            nativeKind: "opencode",
            sessionHandle: "rts_child",
            cwd: "/tmp/project",
            profileFingerprint: "opencode-fingerprint",
            ownership: "readOnly",
            bindingRevision: 3
          },
          controls: [],
          activeSession: {
            sessionHandle: "rts_child",
            threadId,
            title: "Recovered review",
            parentThreadId: "thread-1",
            dedupKey: "rtd_child",
            fidelity: "summary",
            ownership: "readOnly",
            actions: ["read"]
          },
          children: []
        };
      }
      return {
        runtimeRef: "opencode",
        selectionState: "bound",
        profiles: gatewayMock.runtimeProfileRecords,
        binding: {
          threadId: "thread-1",
          runtimeRef: "opencode",
          backendKind: "runtime",
          nativeKind: "opencode",
          sessionHandle: "rts_parent",
          cwd: "/tmp/project",
          profileFingerprint: "opencode-fingerprint",
          ownership: "readWrite",
          bindingRevision: 2
        },
        controls: [],
        activeSession: null,
        children: [{
          sessionHandle: "rts_child",
          threadId: "runtime-child-thread",
          parentThreadId: "thread-1",
          title: "Recovered review",
          dedupKey: "rtd_child",
          fidelity: "partial",
          ownership: "readOnly",
          actions: ["read"]
        }]
      };
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Parent runtime session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    fireEvent.click(await screen.findByRole("button", { name: "Recovered review" }));
    const childPanel = await screen.findByRole("region", { name: "Recovered review" });
    const notice = await within(childPanel).findByRole("note");

    expect(notice.textContent).toContain("Summary history; only a condensed record is available.");
    expect(notice.getAttribute("data-history-fidelity")).toBe("summary");
    expect(gatewayMock.requestLog).toContainEqual({
      method: "runtime/session/read",
      params: {
        runtimeRef: "opencode",
        sessionHandle: "rts_child",
        scope: gatewayMock.scope
      }
    });
  });

  it("registers a stable runtime child as a navigable read-only thread", async () => {
    gatewayMock.runtimeContextRead = (params) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      if (threadId === "runtime-child-thread") {
        return {
          runtimeRef: "opencode",
          selectionState: "bound",
          profiles: gatewayMock.runtimeProfileRecords,
          binding: {
            threadId,
            runtimeRef: "opencode",
            backendKind: "runtime",
            nativeKind: "opencode",
            sessionHandle: "native-child-secret",
            cwd: "/tmp/project",
            profileFingerprint: "opencode-fingerprint",
            ownership: "readOnly",
            bindingRevision: 3
          },
          controls: [],
          activeSession: null
        };
      }
      return {
        runtimeRef: "native",
        selectionState: "default",
        profiles: gatewayMock.runtimeProfileRecords,
        binding: null,
        controls: [],
        activeSession: null
      };
    };

    render(<App />);
    const composer = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "bind the parent thread" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(true);
    });
    await act(async () => {
      gatewayMock.subscribers.forEach((subscriber) => subscriber({
        method: "gateway/event",
        params: {
          type: "runtimeChildChanged",
          runtimeRef: "opencode",
          parentThreadId: "thread-1",
          threadId: "runtime-child-thread",
          dedupKey: "opaque-runtime-child",
          status: "running",
          readOnly: true
        }
      }));
    });

    const childTab = await screen.findByRole("button", { name: "OpenCode child" });
    fireEvent.click(childTab);
    const childPanel = await screen.findByRole("region", { name: "OpenCode child" });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "runtime/context/read",
        params: expect.objectContaining({ threadId: "runtime-child-thread" })
      });
    });
    expect(within(childPanel).getByText(/^Read-only runtime child/)).toBeTruthy();
    expect(within(childPanel).queryByRole("button", { name: "Send message" })).toBeNull();
    expect(within(childPanel).queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
    expect(within(childPanel).queryByPlaceholderText("Ask Psychevo...")).toBeNull();
  });

  it("keeps Agent Definition independent and fails closed for unsupported pairing", async () => {
    gatewayMock.agentRecords = [{
      ...agentRecord("translate", ["subagent"]),
      tools: ["read"],
      contributions: ["instructions", "tools"]
    }];

    render(<App />);

    await selectMainAgent("translate");
    const runtimePopover = await openRuntimeProfilePopover();
    const incompatibleRuntime = within(runtimePopover).getByRole("radio", { name: "OpenCode" }) as HTMLButtonElement;
    expect(incompatibleRuntime.disabled).toBe(true);
    expect(incompatibleRuntime.title).toContain("cannot faithfully apply the required Agent Definition tool policy");
    fireEvent.click(screen.getByRole("button", { name: "Runtime Profile" }));

    await selectMainAgent("");
    await selectRuntime("opencode");
    expect(screen.getByRole("button", { name: "Agent" }).textContent).toContain("Default Agent");
    expect(screen.getByRole("button", { name: "Runtime Profile" }).textContent).toContain("OpenCode");
    const agentPopover = await openAgentRuntimePopover();
    const agentGroup = within(agentPopover).getByRole("radiogroup", { name: "Main agent" });
    expect((within(agentGroup).getByRole("radio", { name: "Default Agent" }) as HTMLButtonElement).disabled).toBe(false);
    const incompatibleAgent = within(agentGroup).getByRole("radio", { name: "translate" }) as HTMLButtonElement;
    expect(incompatibleAgent.disabled).toBe(true);
    expect(incompatibleAgent.title).toContain("cannot faithfully apply the required Agent Definition tool policy");

    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "translate this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: null,
          runtimeRef: "opencode",
          runtimeOptions: {}
        })
      });
    });
  });

  it("pairs a direct Runtime Profile with injectable instructions and explicitly optional tool policy", async () => {
    gatewayMock.agentRecords = [{
      ...agentRecord("translate", ["subagent"]),
      tools: ["read"],
      contributions: ["instructions", "tools"],
      optionalContributions: ["tools"]
    }];

    render(<App />);

    await selectMainAgent("translate");
    await selectRuntime("opencode");
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), { target: { value: "translate this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          agentName: "translate",
          runtimeRef: "opencode"
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
