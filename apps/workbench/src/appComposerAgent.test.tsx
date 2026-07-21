// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TranscriptEntry } from "@psychevo/protocol";
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

async function openRightInspector() {
  const toggle = await screen.findByRole("button", { name: "Right inspector" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
}

function collapseRightInspector() {
  const toggle = screen.getByRole("button", { name: "Right inspector" });
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
}

describe("Workbench layout and workspace panels", () => {
  it("uses the authoritative initialize display path for the cold Composer", async () => {
    const canonicalCwd = "/home/tester/Projects/a-very-long-workspace";
    gatewayMock.scope.cwd = canonicalCwd;
    gatewayMock.threadBrowser = () => ({ workspaces: [] });
    gatewayMock.initialize = () => ({
      server: "test",
      version: "0.0.0",
      cwd: canonicalCwd,
      displayCwd: "~/Projects/a-very-long-workspace",
      scope: gatewayMock.scope,
      source: gatewayMock.source,
      profile: null,
      capabilities: {}
    });

    render(<App />);

    const workspace = await screen.findByRole("button", { name: "Workspace" });
    expect(workspace.textContent).toBe("~/Projects/a-very-long-workspace");
    expect(workspace.getAttribute("title")).toBe(canonicalCwd);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/open",
        params: expect.objectContaining({
          origin: expect.objectContaining({ cwd: canonicalCwd })
        })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "settings/read")).toBe(false);
  });

  it("runs Native full fork from the session row and opens the authoritative child", async () => {
    gatewayMock.sessionSummaries = [{
      ...sessionSummary("thread-1", "Full fork source"),
      lifecycle: {
        targetLabel: "Psychevo (Native)",
        actions: [
          { id: "fork", enabled: true, unavailableReason: null },
          { id: "delete", enabled: true, unavailableReason: null }
        ]
      }
    }];
    gatewayMock.threadActionRun = () => ({
      kind: "fork",
      sourceThreadId: "thread-1",
      snapshot: {
        ...gatewayMock.snapshot,
        thread: {
          id: "fork-child",
          backend: { kind: "native", sessionHandle: null, runtimeRef: "native" },
          sourceKey: "source-fork-child",
          forkedFromThreadId: "thread-1"
        },
        entries: []
      }
    });

    const { container } = render(<App />);
    await screen.findByText("Full fork source");
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    fireEvent.click(screen.getByRole("menuitem", { name: "Fork" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: expect.objectContaining({
          threadId: "thread-1",
          action: { kind: "fork" }
        })
      });
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "fork-child" })
      });
    });
  });

  it("deletes the idle current session and returns to an empty draft", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Current idle session")];
    const { container } = render(<App />);

    fireEvent.click(await screen.findByText("Current idle session"));
    await waitFor(() => expect(container.querySelector(".pevo-sessionRow.is-active")).toBeTruthy());
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    const deleteAction = screen.getByRole("menuitem", { name: "Delete" }) as HTMLButtonElement;
    expect(deleteAction.disabled).toBe(false);
    fireEvent.click(deleteAction);
    fireEvent.click(within(screen.getByRole("dialog", { name: "Delete session?" }))
      .getByRole("button", { name: "Delete session" }));

    await waitFor(() => {
      const methods = gatewayMock.requestLog.map((entry) => entry.method);
      const deleteIndex = methods.lastIndexOf("thread/delete");
      const nextStartIndex = methods.findIndex((method, index) => index > deleteIndex && method === "thread/draft/open");
      expect(deleteIndex).toBeGreaterThan(-1);
      expect(nextStartIndex).toBeGreaterThan(deleteIndex);
    });
    await waitFor(() => expect(container.querySelector(".pevo-sessionRow.is-active")).toBeNull());
  });

  it("keeps missing fork provenance visible but disables source navigation", async () => {
    gatewayMock.sessionSummaries = [{
      ...sessionSummary("fork-child", "Detached fork"),
      forkedFromThreadId: "deleted-source-thread"
    }];
    gatewayMock.snapshot.thread = {
      id: "fork-child",
      backend: { kind: "native", sessionHandle: "fork-child", runtimeRef: "native" },
      sourceKey: "source-fork-child",
      forkedFromThreadId: "deleted-source-thread"
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Detached fork"));

    const provenance = await screen.findByRole("button", { name: "Forked from deleted-" });
    expect((provenance as HTMLButtonElement).disabled).toBe(true);
    expect(provenance.getAttribute("title")).toContain("deleted-source-thread is unavailable");
  });

  it("keeps the inline edit available when turn admission fails and retries the same staged draft", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Retry history edit")];
    (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [userTextEntry("Original prompt")];
    gatewayMock.threadHistoryDraftRead = () => ({
      threadId: "thread-1",
      messageId: "message:1",
      messageSeq: 1,
      parts: [{ type: "text", text: "Original prompt" }],
      fidelity: "exact",
      warning: null,
      unavailableReason: null
    });
    gatewayMock.threadActionRun = () => ({
      kind: "revertConversation",
      threadId: "thread-1",
      staged: true,
      noOp: false,
      snapshot: {
        ...gatewayMock.snapshot,
        entries: [],
        historyEditing: {
          kind: "conversationEdit",
          boundaryMessageId: "message:1",
          hiddenEntryCount: 1,
          replacementDraft: { parts: [{ type: "text", text: "Edited prompt" }] },
          availableActions: ["restoreHistory"]
        }
      }
    });
    let turnAttempts = 0;
    gatewayMock.turnStart = () => {
      turnAttempts += 1;
      if (turnAttempts === 1) {
        throw new Error("The selected model became unavailable.");
      }
      return {
        accepted: true,
        threadId: "thread-1",
        turnId: "turn:thread-1",
        thread: gatewayMock.snapshot.thread
      };
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Retry history edit"));
    await screen.findByText("Original prompt");
    fireEvent.click(await screen.findByRole("button", { name: /Edit this message/ }));
    const editor = await screen.findByRole("textbox", { name: "Message text 1" });
    fireEvent.change(editor, { target: { value: "Edited prompt" } });
    const update = screen.getByRole("button", { name: "Update this message and run in the same thread" });
    fireEvent.click(update);

    expect(await screen.findByText("The selected model became unavailable.")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Message text 1" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Update this message and run in the same thread" }));

    await waitFor(() => expect(turnAttempts).toBe(2));
    expect(gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/action/run"
      && (entry.params as { action?: { kind?: string } }).action?.kind === "revertConversation"
    ))).toHaveLength(2);
    await waitFor(() => {
      expect(screen.queryByRole("textbox", { name: "Message text 1" })).toBeNull();
    });
  });

  it("allows a completed replacement message to be updated and run again", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Repeat history edit")];
    (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [userTextEntry("Original prompt")];
    let editableText = "Original prompt";
    let turnRunning = false;
    let runningContextReads = 0;
    const turnIds: string[] = [];

    gatewayMock.runtimeContextRead = () => {
      if (turnRunning) runningContextReads += 1;
      return nativeHistoryEditingContext(!turnRunning);
    };
    gatewayMock.threadHistoryDraftRead = (params) => ({
      threadId: "thread-1",
      messageId: (params as { messageId: string }).messageId,
      messageSeq: 1,
      parts: [{ type: "text", text: editableText }],
      fidelity: "exact",
      warning: null,
      unavailableReason: null
    });
    gatewayMock.threadActionRun = (params) => {
      const action = (params as {
        action: {
          kind: "revertConversation";
          messageId: string;
          draft: { parts: Array<{ type: "text"; text: string }> };
        };
      }).action;
      editableText = action.draft.parts.map((part) => part.text).join("\n");
      return {
        kind: "revertConversation",
        threadId: "thread-1",
        staged: true,
        noOp: false,
        snapshot: {
          ...gatewayMock.snapshot,
          entries: [],
          historyEditing: {
            kind: "conversationEdit",
            boundaryMessageId: action.messageId,
            hiddenEntryCount: 1,
            replacementDraft: action.draft,
            availableActions: ["restoreHistory"]
          }
        }
      };
    };
    gatewayMock.turnStart = () => {
      turnRunning = true;
      const turnId = `turn:thread-1:${turnIds.length + 1}`;
      turnIds.push(turnId);
      return {
        accepted: true,
        threadId: "thread-1",
        turnId,
        thread: gatewayMock.snapshot.thread
      };
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Repeat history edit"));
    await screen.findByText("Original prompt");

    fireEvent.click(await screen.findByRole("button", { name: /Edit this message/ }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Message text 1" }), {
      target: { value: "First replacement" }
    });
    fireEvent.click(screen.getByRole("button", {
      name: "Update this message and run in the same thread"
    }));

    await waitFor(() => expect(turnIds).toHaveLength(1));
    await waitFor(() => expect(runningContextReads).toBeGreaterThan(0));
    await emitGatewayEvent({
      type: "turnStarted",
      threadId: "thread-1",
      turnId: turnIds[0],
      selectedSkills: []
    });
    turnRunning = false;
    await emitGatewayEvent(completedTurnEvent(
      turnIds[0]!,
      userTextEntryForTurn("First replacement", turnIds[0]!)
    ));

    expect(await screen.findByText("First replacement")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: /Edit this message/ }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Message text 1" }), {
      target: { value: "Second replacement" }
    });
    fireEvent.click(screen.getByRole("button", {
      name: "Update this message and run in the same thread"
    }));

    await waitFor(() => expect(turnIds).toHaveLength(2));
    expect(gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/action/run"
      && (entry.params as { action?: { kind?: string } }).action?.kind === "revertConversation"
    ))).toHaveLength(2);
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "turn/start")).toHaveLength(2);
  });

  it("restores staged conversation history and keeps the ordered replacement draft in Composer", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "History editing")];
    gatewayMock.snapshot.historyEditing = {
      kind: "conversationEdit",
      boundaryMessageId: "message:1",
      hiddenEntryCount: 2,
      replacementDraft: {
        parts: [
          { type: "text", text: "edited before" },
          { type: "image", input: { kind: "url", url: "https://example.test/history.png" } },
          { type: "text", text: "edited after" }
        ]
      },
      availableActions: ["restoreHistory"]
    };
    gatewayMock.threadActionRun = () => ({
      kind: "unrevertConversation",
      threadId: "thread-1",
      snapshot: { ...gatewayMock.snapshot, historyEditing: null },
      draft: gatewayMock.snapshot.historyEditing?.replacementDraft
    });

    render(<App />);
    fireEvent.click(await screen.findByText("History editing"));
    expect(await screen.findByText("2 hidden entries")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restore history" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: expect.objectContaining({
          threadId: "thread-1",
          action: { kind: "unrevertConversation" }
        })
      });
    });
    expect((screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement).value).toBe(
      "edited before\nedited after"
    );
    expect(screen.getByText("history.png")).toBeTruthy();
  });

  it("requests initial history once and suppresses the empty state until it resolves", async () => {
    const browser = deferred<Record<string, unknown>>();
    gatewayMock.threadBrowser = () => browser.promise;

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/browser")).toHaveLength(1);
    });
    expect(screen.getByRole("region", { name: "Sessions" }).getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByText("No sessions")).toBeNull();

    browser.resolve({ workspaces: [] });
    expect(await screen.findByText("No sessions")).toBeTruthy();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/browser")).toHaveLength(1);
  });

  it("starts in a hidden draft without rendering a history draft row", async () => {
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    expect(container.querySelector(".conversationColumn")?.classList.contains("is-draftSession")).toBe(true);
    expect((container.querySelector(".workbench") as HTMLElement | null)?.style.getPropertyValue("--right-column-width")).toBe("520px");
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/open",
        params: expect.objectContaining({ origin: gatewayMock.scope })
      });
    });
    expect(container.querySelectorAll(".pevo-sessionRow.is-draft")).toHaveLength(0);
    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
    const status = screen.getByLabelText("Composer environment");
    await waitFor(() => {
      expect(within(status).getByRole("combobox", { name: "Permission mode" })).toBeTruthy();
    });
    const permission = within(status).getByRole("combobox", { name: "Permission mode" });
    const workspace = within(status).getByRole("button", { name: "Workspace" });
    const branch = within(status).getByRole("button", { name: "Git branch" });
    expect(permission.tagName).toBe("BUTTON");
    expect(workspace.compareDocumentPosition(branch) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(branch.compareDocumentPosition(
      permission
    ) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(branch.textContent).toContain(gatewayMock.projectBranch);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "workspace/git/branches"))
        .toHaveLength(1);
    });
    expect(container.querySelector(".composerRuntimeControls")?.querySelector('[aria-label="Permission mode"]')).toBeNull();
  });

  it("retains the committed Composer environment while New Session opens", async () => {
    Object.assign(gatewayMock.scope.source, {
      rawId: "workspace-source",
      rawIdentity: "workspace-source-identity",
      visibleName: "Workspace source"
    });
    render(<App />);

    const environment = await waitFor(() => {
      const value = {
        agent: screen.getByRole("button", { name: "Agent target" }).textContent,
        branch: screen.getByRole("button", { name: "Git branch" }).textContent,
        permission: screen.getByRole("combobox", { name: "Permission mode" }).textContent,
        workspace: screen.getByRole("button", { name: "Workspace" }).textContent
      };
      expect(value.agent).toContain("Psychevo");
      expect(value.branch).toContain(gatewayMock.projectBranch);
      return value;
    });
    const draftOpen = deferred<Record<string, unknown>>();
    const branches = deferred<Record<string, unknown>>();
    gatewayMock.draftOpen = () => draftOpen.promise;
    gatewayMock.workspaceGitBranches = () => branches.promise;

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(2);
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open").at(-1))
      .toEqual({
        method: "thread/draft/open",
        params: expect.objectContaining({ origin: gatewayMock.scope })
      });
    expect({
      agent: screen.getByRole("button", { name: "Agent target" }).textContent,
      branch: screen.getByRole("button", { name: "Git branch" }).textContent,
      permission: screen.getByRole("combobox", { name: "Permission mode" }).textContent,
      workspace: screen.getByRole("button", { name: "Workspace" }).textContent
    }).toEqual(environment);
    expect((screen.getByRole("combobox", { name: "Permission mode" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "Workspace" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "Git branch" }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it("moves the same Composer from center stage to the bottom after the first accepted prompt", async () => {
    const { container } = render(<App />);
    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    const conversation = container.querySelector(".conversationColumn") as HTMLElement;
    const dock = container.querySelector(".composerDock") as HTMLElement & {
      animate: ReturnType<typeof vi.fn>;
      getAnimations: ReturnType<typeof vi.fn>;
    };
    const animate = vi.fn();
    Object.defineProperties(dock, {
      animate: { configurable: true, value: animate },
      getAnimations: { configurable: true, value: vi.fn(() => []) },
      getBoundingClientRect: {
        configurable: true,
        value: vi.fn(() => ({
          bottom: conversation.classList.contains("is-draftSession") ? 280 : 680,
          height: 80,
          left: 100,
          right: 700,
          top: conversation.classList.contains("is-draftSession") ? 200 : 600,
          width: 600,
          x: 100,
          y: conversation.classList.contains("is-draftSession") ? 200 : 600,
          toJSON: () => ({})
        }))
      }
    });

    fireEvent.change(textarea, { target: { value: "Start the thread" } });
    const send = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    await waitFor(() => expect(send.disabled).toBe(false));
    fireEvent.click(send);

    await waitFor(() => expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(true));
    await waitFor(() => expect(conversation.classList.contains("is-draftSession")).toBe(false));
    expect(container.querySelector(".composerDock")).toBe(dock);
    expect(animate).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ transform: expect.stringContaining("translate") }),
        { transform: "translate(0, 0)" }
      ]),
      expect.objectContaining({ duration: 360 })
    );
  });

  it("switches draft workspaces and exposes the open workspace action last", async () => {
    const otherCwd = "/home/tester/Projects/a-very-long-workspace";
    const otherDisplayPath = "~/Projects/a-very-long-workspace";
    gatewayMock.browserWorkspaces = [
      {
        cwd: "/tmp/project",
        project: { cwd: "/tmp/project", label: "project", displayPath: "/tmp/project" },
        sessions: [],
        hiddenCount: 0,
        nextCursor: null
      },
      {
        cwd: otherCwd,
        project: { cwd: otherCwd, label: "a-very-long-workspace", displayPath: otherDisplayPath },
        sessions: [],
        hiddenCount: 0,
        nextCursor: null
      }
    ];
    render(<App />);

    const workspace = await screen.findByRole("button", { name: "Workspace" });
    fireEvent.click(workspace);
    const menu = await screen.findByRole("menu", { name: "Workspace" });
    const items = within(menu).getAllByRole("menuitem");
    expect(items.at(-1)?.textContent).toContain("Open workspace...");
    fireEvent.click(within(menu).getByRole("menuitem", { name: otherDisplayPath }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/open",
        params: expect.objectContaining({ origin: expect.objectContaining({ cwd: otherCwd }) })
      });
      expect(screen.getByRole("button", { name: "Workspace" }).textContent)
        .toBe(otherDisplayPath);
    });
    expect(screen.getByRole("button", { name: "Workspace" }).getAttribute("title")).toBe(otherCwd);

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open workspace..." }));
    const dialog = await screen.findByRole("dialog", { name: "Choose workspace folder" });
    const location = within(dialog).getByRole("textbox", { name: "Folder path" }) as HTMLInputElement;
    expect(location.value).toBe("/tmp");
    fireEvent.click(await within(dialog).findByRole("button", { name: "manual-project" }));
    await waitFor(() => expect(location.value).toBe("/tmp/manual-project"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Open folder" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/open",
        params: expect.objectContaining({ origin: expect.objectContaining({ cwd: "/tmp/manual-project" }) })
      });
    });
  });

  it("opens or creates a workspace from the Sessions header folder panel", async () => {
    gatewayMock.workspaceFolderList = (params) => {
      const current = (params as { path?: string | null }).path ?? "/tmp/project";
      return {
        root: "/",
        roots: [{ name: "/", path: "/" }],
        current,
        parent: current === "/" ? null : "/tmp",
        folders: current === "/tmp/project"
          ? [{ name: "existing-workspace", path: "/tmp/project/existing-workspace" }]
          : []
      };
    };
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open workspace" }));
    const dialog = await screen.findByRole("dialog", { name: "Open workspace" });
    const location = within(dialog).getByRole("textbox", { name: "Folder path" }) as HTMLInputElement;
    expect(location.value).toBe("/tmp/project");
    fireEvent.click(await within(dialog).findByRole("button", { name: "existing-workspace" }));
    await waitFor(() => expect(location.value).toBe("/tmp/project/existing-workspace"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Open folder" }));
    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/draft/open",
      params: expect.objectContaining({ origin: expect.objectContaining({ cwd: "/tmp/project/existing-workspace" }) })
    }));

    fireEvent.click(await screen.findByRole("button", { name: "Open workspace" }));
    const createDialog = await screen.findByRole("dialog", { name: "Open workspace" });
    fireEvent.click(await within(createDialog).findByRole("button", { name: "New workspace..." }));
    fireEvent.change(within(createDialog).getByRole("textbox", { name: "Workspace name" }), { target: { value: "fresh-workspace" } });
    fireEvent.click(within(createDialog).getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "workspace/create",
      params: { name: "fresh-workspace", parent: "/tmp/project" }
    }));
  });

  it("browses and opens custom paths pasted into the workspace folder location", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open workspace" }));
    const dialog = await screen.findByRole("dialog", { name: "Open workspace" });
    const location = within(dialog).getByRole("textbox", { name: "Folder path" }) as HTMLInputElement;

    fireEvent.change(location, { target: { value: "/opt/pasted-workspace" } });
    fireEvent.keyDown(location, { key: "Enter" });
    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "workspace/folders",
      params: expect.objectContaining({ path: "/opt/pasted-workspace" })
    }));
    expect(location.value).toBe("/opt/pasted-workspace");

    fireEvent.change(location, { target: { value: "/srv/direct-open-workspace" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Open folder" }));
    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/draft/open",
      params: expect.objectContaining({ origin: expect.objectContaining({ cwd: "/srv/direct-open-workspace" }) })
    }));
  });

  it("opens a workspace on another Windows drive from the folder panel", async () => {
    gatewayMock.workspaceFolderList = (params) => {
      const current = (params as { path?: string | null }).path ?? "C:\\project";
      const root = current.startsWith("D:") ? "D:\\" : "C:\\";
      return {
        root,
        roots: [
          { name: "C:", path: "C:\\" },
          { name: "D:", path: "D:\\" }
        ],
        current,
        parent: current === root ? null : root,
        folders: current === "D:\\"
          ? [{ name: "other-workspace", path: "D:\\other-workspace" }]
          : []
      };
    };
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open workspace" }));
    const dialog = await screen.findByRole("dialog", { name: "Open workspace" });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Drive" }), {
      target: { value: "D:\\" }
    });
    fireEvent.click(await within(dialog).findByRole("button", { name: "other-workspace" }));
    await waitFor(() => expect((within(dialog).getByRole("textbox", { name: "Folder path" }) as HTMLInputElement).value)
      .toBe("D:\\other-workspace"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Open folder" }));

    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/draft/open",
      params: expect.objectContaining({ origin: expect.objectContaining({ cwd: "D:\\other-workspace" }) })
    }));
  });

  it("switches and creates Git branches from the Composer environment menu", async () => {
    render(<App />);

    const branch = await screen.findByRole("button", { name: "Git branch" });
    fireEvent.click(branch);
    const menu = await screen.findByRole("menu", { name: "Git branch" });
    expect(within(menu).getAllByRole("menuitem").at(-1)?.textContent).toContain("New branch...");
    fireEvent.click(within(menu).getByRole("menuitem", { name: "feature/composer" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/git/checkout",
        params: expect.objectContaining({ branch: "feature/composer", create: false })
      });
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Git branch" }).textContent).toContain("feature/composer"));

    fireEvent.click(screen.getByRole("button", { name: "Git branch" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "New branch..." }));
    const dialog = await screen.findByRole("dialog", { name: "New branch" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Branch name" }), {
      target: { value: "feature/new-composer" }
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create branch" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/git/checkout",
        params: expect.objectContaining({ branch: "feature/new-composer", create: true })
      });
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Git branch" }).textContent).toContain("feature/new-composer"));
  });

  it("requires explicit controls to dismiss workspace and branch dialogs", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open workspace" }));
    const workspaceDialog = await screen.findByRole("dialog", { name: "Open workspace" });
    fireEvent.mouseDown(workspaceDialog.parentElement!);
    expect(screen.getByRole("dialog", { name: "Open workspace" })).toBeTruthy();
    fireEvent.click(within(workspaceDialog).getByRole("button", { name: "Cancel" }));

    fireEvent.click(await screen.findByRole("button", { name: "Git branch" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "New branch..." }));
    const branchDialog = await screen.findByRole("dialog", { name: "New branch" });
    fireEvent.mouseDown(branchDialog.parentElement!);
    expect(screen.getByRole("dialog", { name: "New branch" })).toBeTruthy();
  });

  it("keeps the bound Workspace control scoped to Files instead of retargeting the Thread", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Bound workspace")];
    render(<App />);

    fireEvent.click(await screen.findByText("Bound workspace"));
    const workspace = await screen.findByRole("button", { name: "Workspace" });
    fireEvent.click(workspace);

    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();
    expect(screen.queryByRole("menu", { name: "Workspace" })).toBeNull();
  });

  it("opens right workspace tabs from Home and the add menu", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    expect(within(home).queryByText("local PTY")).toBeNull();
    expect(within(home).queryByText("workspace tree")).toBeNull();
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    fireEvent.click(document.querySelector(".rightAddMenu summary") as HTMLElement);
    const addMenuFiles = screen.getAllByRole("menuitem", { name: "Files" }).at(-1);
    expect(addMenuFiles).toBeTruthy();
    fireEvent.click(addMenuFiles!);
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "Workspace home" }));
    const visibleHome = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(visibleHome).getByRole("button", { name: /Terminal/ }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("opens a reusable preview-only Browser tab with safe URL handling", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Browser session")];
    render(<App />);

    fireEvent.click(await screen.findByText("Browser session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    const browser = await screen.findByRole("region", { name: "Browser" });

    const annotate = within(browser).getByLabelText("Annotate page") as HTMLButtonElement;
    expect(annotate.disabled).toBe(true);
    expect(annotate.getAttribute("title")).toBe("Desktop required");

    const openUrl = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(openUrl, { target: { value: "file:///tmp/page.html" } });
    fireEvent.submit(openUrl.closest("form") as HTMLFormElement);
    expect((await within(browser).findByRole("alert")).textContent).toContain("Browser supports http and https URLs.");

    fireEvent.change(openUrl, { target: { value: "example.com:8080" } });
    fireEvent.submit(openUrl.closest("form") as HTMLFormElement);
    const iframe = await within(browser).findByTitle("example.com") as HTMLIFrameElement;
    expect(iframe.getAttribute("src")).toBe("https://example.com:8080/");
    expect(iframe.hasAttribute("sandbox")).toBe(false);
    expect(within(browser).getByText("Preview only")).toBeTruthy();

    const address = within(browser).getByLabelText("Browser address") as HTMLInputElement;
    for (const [input, title, expected] of [
      ["localhost:3000", "localhost", "http://localhost:3000/"],
      ["127.0.0.1:9222", "127.0.0.1", "http://127.0.0.1:9222/"],
      ["[::1]:4173", "[::1]", "http://[::1]:4173/"]
    ] as const) {
      fireEvent.change(address, { target: { value: input } });
      fireEvent.submit(address.closest("form") as HTMLFormElement);
      expect((await within(browser).findByTitle(title)).getAttribute("src")).toBe(expected);
    }

    fireEvent.change(address, { target: { value: "vscode://file/tmp/page.html" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByRole("alert")).textContent).toContain("Browser supports http and https URLs.");

    fireEvent.click(screen.getByRole("tab", { name: "Workspace home" }));
    const visibleHome = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(visibleHome).getByRole("button", { name: "Browser" }));
    expect(within(screen.getByRole("tablist", { name: "Right workspace tabs" }))
      .getAllByRole("tab", { name: "Browser" })).toHaveLength(1);
  });

  it("isolates Browser tabs and restores navigation state per thread", async () => {
    gatewayMock.sessionSummaries = [
      sessionSummary("thread-a", "Thread A"),
      sessionSummary("thread-b", "Thread B")
    ];
    render(<App />);

    fireEvent.click(await screen.findByText("Thread A"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-a" })
      });
    });
    await openRightInspector();
    let home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    let browser = await screen.findByRole("region", { name: "Browser" });
    let address = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(address, { target: { value: "a.example" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByTitle("a.example")).getAttribute("src")).toBe("https://a.example/");

    fireEvent.click(screen.getByText("Thread B"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-b" })
      });
    });
    home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    browser = await screen.findByRole("region", { name: "Browser" });
    expect(within(browser).queryByTitle("a.example")).toBeNull();
    address = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(address, { target: { value: "b.example" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByTitle("b.example")).getAttribute("src")).toBe("https://b.example/");
    expect(within(screen.getByRole("tablist", { name: "Right workspace tabs" }))
      .getAllByRole("tab", { name: "Browser" })).toHaveLength(1);

    fireEvent.click(screen.getByText("Thread A"));
    home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    browser = await screen.findByRole("region", { name: "Browser" });
    expect((within(browser).getByLabelText("Browser address") as HTMLInputElement).value).toBe("https://a.example/");
    expect((within(browser).getByTitle("a.example") as HTMLIFrameElement).getAttribute("src")).toBe("https://a.example/");
    expect(within(browser).queryByTitle("b.example")).toBeNull();
    expect(within(screen.getByRole("tablist", { name: "Right workspace tabs" }))
      .getAllByRole("tab", { name: "Browser" })).toHaveLength(1);
  });

  it("closes the right workspace add menu on outside click and item activation", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    const trigger = document.querySelector(".rightAddMenu summary") as HTMLElement | null;
    const menu = trigger!.closest("details") as HTMLDetailsElement | null;
    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.mouseDown(screen.getByRole("region", { name: "Transcript" }));
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Files" }));
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Terminal" }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("restores and clamps the right workspace width preference", async () => {
    window.localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({
      appearance: "dark",
      debug: false,
      rightWidthPx: 9999
    }));
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    const workbench = container.querySelector(".workbench") as HTMLElement | null;
    expect(workbench?.style.getPropertyValue("--right-column-width")).toBe("1200px");
  });

  it("toggles Review changed files and scopes the diff preview", async () => {
    gatewayMock.workspaceDiffResult = {
      isGitRepo: true,
      files: [
        { path: "docs/api.md", status: "modified", binary: false, unreadable: false, placeholder: null },
        { path: "src/main.rs", status: "modified", binary: false, unreadable: false, placeholder: null }
      ],
      unifiedDiff: [
        "diff --git a/docs/api.md b/docs/api.md",
        "--- a/docs/api.md",
        "+++ b/docs/api.md",
        "@@ -1 +1 @@",
        "-old docs",
        "+new docs",
        "diff --git a/src/main.rs b/src/main.rs",
        "--- a/src/main.rs",
        "+++ b/src/main.rs",
        "@@ -1 +1 @@",
        "-old main",
        "+new main"
      ].join("\n"),
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });
    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    expect(within(review).getAllByText("M↓").length).toBeGreaterThan(0);
    expect(within(review).getAllByLabelText("1 additions, 1 deletions").length).toBeGreaterThan(0);
    expect(within(review).queryByText("diff --git a/docs/api.md b/docs/api.md")).toBeNull();

    fireEvent.click(within(review).getByRole("button", { name: "Show changed files" }));
    expect(within(review).getByLabelText("Filter changed files")).toBeTruthy();
    fireEvent.change(within(review).getByLabelText("Filter changed files"), { target: { value: "main" } });
    expect(within(review).getByRole("treeitem", { name: /main\.rs/ })).toBeTruthy();
    expect(within(review).queryByRole("treeitem", { name: /api\.md/ })).toBeNull();

    fireEvent.click(within(review).getByRole("treeitem", { name: /main\.rs/ }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/diff",
        params: expect.objectContaining({ path: "src/main.rs" })
      });
    });
    expect(await within(review).findByText("new selected")).toBeTruthy();
    expect(within(review).getByText("src/main.rs")).toBeTruthy();
  });

  it("rejects turn-scoped Review files through workspace change RPCs", async () => {
    gatewayMock.workspaceChangesResult = {
      groups: [
        {
          turnId: "turn-1",
          threadId: "thread-1",
          createdAtMs: 1,
          completedAtMs: 2,
          files: [
            {
              path: "docs/api.md",
              status: "modified",
              binary: false,
              unreadable: false,
              reviewStatus: "pending",
              canReject: true,
              message: null
            }
          ]
        }
      ]
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });

    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    fireEvent.click(within(review).getByLabelText("Reject docs/api.md"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/change/reject",
        params: expect.objectContaining({ path: "docs/api.md", turnId: "turn-1" })
      });
    });
  });

  it("renders Markdown file previews from the shared Markdown component", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 },
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.rs", name: "main.rs", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const markdownSource = "---\ntitle: API Notes\ntags:\n  - docs\n  - guide\n---\n# API Notes\n\n- supports markdown";
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: markdownSource,
      binary: false,
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    expect(within(files).getByLabelText("Filter workspace files")).toBeTruthy();
    expect(files.querySelector("header p")).toBeNull();

    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    const breadcrumb = await within(files).findByRole("navigation", { name: "File breadcrumb" });
    expect(breadcrumb.textContent).toBe("projectdocsREADME.md");
    expect(within(files).queryByText("/tmp/project/docs/README.md")).toBeNull();
    const table = await within(files).findByRole("table", { name: "YAML frontmatter" });
    expect(within(table).getByText("title")).toBeTruthy();
    expect(within(table).getByText("docs")).toBeTruthy();
    expect(within(table).getByText("guide")).toBeTruthy();
    expect(await within(files).findByRole("heading", { name: "API Notes" })).toBeTruthy();
    expect(within(files).getByText("supports markdown")).toBeTruthy();
    fireEvent.click(within(files).getByRole("button", { name: "Copy docs/README.md" }));
    await waitFor(() => {
      expect(gatewayMock.clipboardWriteLog[gatewayMock.clipboardWriteLog.length - 1]).toBe(markdownSource);
    });
    const sourceViewToggle = within(files).getByRole("button", { name: "Source view for docs/README.md" });
    expect(sourceViewToggle.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(sourceViewToggle);
    expect(sourceViewToggle.getAttribute("aria-pressed")).toBe("true");
    expect(files.querySelector(".rightCodePreview")?.textContent).toContain(markdownSource);
    expect(within(files).queryByRole("heading", { name: "API Notes" })).toBeNull();
    fireEvent.click(sourceViewToggle);
    expect(sourceViewToggle.getAttribute("aria-pressed")).toBe("false");
    expect(await within(files).findByRole("heading", { name: "API Notes" })).toBeTruthy();
  });

  it("opens file-tree external actions and keeps rich-preview files context-menu accessible", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "README.md", name: "README.md", kind: "file", depth: 0 },
        { path: "assets", name: "assets", kind: "directory", depth: 0 },
        { path: "assets/photo.png", name: "photo.png", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    const readme = within(files).getByRole("treeitem", { name: /README\.md/ });

    fireEvent.contextMenu(readme, { clientX: 32, clientY: 48 });
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/file/externalActions",
        params: { path: "README.md", scope: gatewayMock.scope }
      });
    });
    expect(await screen.findByRole("menuitem", { name: "Open in VS Code" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Open with Default Application" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Show in Finder" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitem", { name: "Open in VS Code" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/file/openExternal",
        params: { action: "vscode", path: "README.md", scope: gatewayMock.scope }
      });
    });
    await waitFor(() => expect(screen.queryByRole("menu", { name: "Actions for README.md" })).toBeNull());
    expect(document.activeElement).toBe(readme);

    const image = within(files).getByRole("treeitem", { name: /photo\.png/ });
    expect((image as HTMLButtonElement).disabled).toBe(false);
    expect(image.hasAttribute("aria-disabled")).toBe(false);
    expect(image.getAttribute("aria-describedby")).toBeNull();
    fireEvent.contextMenu(image, { clientX: 40, clientY: 56 });
    expect(await screen.findByRole("menuitem", { name: "Open in Default Image Viewer" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Show in Finder" })).toBeTruthy();
  });

  it("opens assistant file links preview-focused and restores the tree from the Composer browser entry", async () => {
    gatewayMock.scope.cwd = "C:\\repo";
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Artifact links", gatewayMock.scope.cwd)];
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "site", name: "site", kind: "directory", depth: 0 },
        { path: "site/index.html", name: "index.html", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const htmlSource = "<!doctype html><html><body><h1>Artifact preview</h1></body></html>";
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: htmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: htmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [assistantTextEntry([
      "Relative site/index.html",
      "Windows C:\\repo\\site\\index.html",
      "Git Bash /c/repo/site/index.html"
    ].join("\n\n"))];

    render(<App />);
    fireEvent.click(await screen.findByText("Artifact links"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    const pathButton = (label: string): HTMLButtonElement => {
      return screen.getByRole("button", { name: `Open file ${label}` }) as HTMLButtonElement;
    };
    await screen.findByRole("button", { name: "Open file site/index.html" });
    expect(pathButton("C:\\repo\\site\\index.html")).toBeTruthy();
    expect(pathButton("/c/repo/site/index.html")).toBeTruthy();

    fireEvent.click(pathButton("site/index.html"));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    await waitFor(() => {
      expect(files.querySelector('iframe[title="site/index.html"]')).toBeInstanceOf(HTMLIFrameElement);
    });
    const frame = files.querySelector('iframe[title="site/index.html"]');
    if (!(frame instanceof HTMLIFrameElement)) {
      throw new Error("missing interactive HTML artifact preview");
    }
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("srcdoc")).toContain("Artifact preview");
    expect(within(files).getByRole("button", { name: "File tree" }).getAttribute("aria-pressed")).toBe("false");
    expect(within(files).queryByRole("complementary", { name: "Workspace file tree" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    expect(within(files).getByRole("button", { name: "File tree" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(files).getByRole("complementary", { name: "Workspace file tree" })).toBeTruthy();

    fireEvent.click(pathButton("C:\\repo\\site\\index.html"));
    fireEvent.click(pathButton("/c/repo/site/index.html"));
    await waitFor(() => {
      const openedPaths = gatewayMock.requestLog
        .filter((entry) => entry.method === "workspace/file/preview/open")
        .map((entry) => (entry.params as { path?: string }).path);
      expect(openedPaths).toEqual(["site/index.html"]);
    });
  });

  it.each(["read", "edit", "write"] as const)(
    "opens a completed %s path in the preview-focused Files layout",
    async (toolName) => {
      gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Tool file target")];
      gatewayMock.workspaceFilesResult = {
        root: gatewayMock.scope.cwd,
        entries: [
          { path: "docs", name: "docs", kind: "directory", depth: 0 },
          { path: "docs/report.md", name: "report.md", kind: "file", depth: 1 }
        ],
        truncated: false
      };
      gatewayMock.workspaceFileReadResults.set("docs/report.md", {
        path: "docs/report.md",
        content: "# Tool report",
        binary: false,
        editable: true,
        editableReason: null,
        revision: "r1",
        sizeBytes: 13,
        lineEnding: "lf",
        unreadable: null,
        truncated: false
      });
      (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [assistantToolEntry(toolName, "docs/report.md")];

      render(<App />);
      fireEvent.click(await screen.findByText("Tool file target"));
      const openFile = await screen.findByRole("button", { name: "Open file docs/report.md" });
      fireEvent.click(openFile);

      const files = await screen.findByRole("region", { name: "Workspace files" });
      expect(await within(files).findByRole("heading", { name: "Tool report" })).toBeTruthy();
      expect(within(files).getByRole("button", { name: "File tree" }).getAttribute("aria-pressed")).toBe("false");
      expect(within(files).queryByRole("complementary", { name: "Workspace file tree" })).toBeNull();
      await waitFor(() => {
        expect(gatewayMock.requestLog).toContainEqual({
          method: "workspace/file/preview/open",
          params: expect.objectContaining({ path: "docs/report.md" })
        });
      });
    }
  );

  it("refreshes a cached same-workspace inventory when a hidden Files tool entry completes", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Generated file")];
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [],
      truncated: false
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Generated file"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    await screen.findByRole("region", { name: "Workspace files" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "workspace/files")).toBe(true);
    });
    collapseRightInspector();

    const emitCompletedTurn = async (turnId: string, entries: TranscriptEntry[]) => {
      await act(async () => {
        for (const subscriber of gatewayMock.subscribers) {
          subscriber({
            method: "gateway/event",
            params: {
              type: "turnCompleted",
              threadId: "thread-1",
              turnId,
              turn: {
                id: turnId,
                threadId: "thread-1",
                status: "completed",
                outcome: "normal",
                error: null,
                startedAtMs: 1,
                completedAtMs: 2
              },
              committedEntries: entries
            }
          });
        }
        await Promise.resolve();
      });
    };

    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "generated", name: "generated", kind: "directory", depth: 0 },
        { path: "generated/result.md", name: "result.md", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const createRequestCount = gatewayMock.requestLog.filter((entry) => (
      entry.method === "workspace/files"
    )).length;
    const writeEntry = {
      ...assistantToolEntry("write", "generated/result.md"),
      turnId: "turn-write"
    };
    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnStarted",
            threadId: "thread-1",
            turnId: "turn-write",
            selectedSkills: []
          }
        });
      }
      await Promise.resolve();
    });
    const animationFrames: FrameRequestCallback[] = [];
    const requestAnimationFrame = vi.spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        animationFrames.push(callback);
        return animationFrames.length;
      });
    try {
      await act(async () => {
        for (const subscriber of gatewayMock.subscribers) {
          subscriber({
            method: "gateway/event",
            params: {
              type: "entryCompleted",
              turnId: "turn-write",
              entry: writeEntry
            }
          });
        }
        for (const frame of animationFrames.splice(0)) {
          frame(0);
        }
        await Promise.resolve();
      });
    } finally {
      requestAnimationFrame.mockRestore();
    }

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "workspace/files"
      )).length).toBe(createRequestCount + 1);
    });
    expect(await screen.findByRole("button", { name: "Open file generated/result.md" })).toBeTruthy();

    await emitCompletedTurn("turn-write", [writeEntry]);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "workspace/files"
      )).length).toBe(createRequestCount + 2);
    });

    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [],
      truncated: false
    };
    const deleteRequestCount = gatewayMock.requestLog.filter((entry) => (
      entry.method === "workspace/files"
    )).length;
    const removalEntry = {
      ...assistantTextEntry("Removed generated/result.md."),
      id: "remove-entry",
      turnId: "turn-remove",
      messageSeq: 2,
      blocks: [{
        ...assistantTextEntry("Removed generated/result.md.").blocks[0]!,
        id: "remove-block",
        body: "Removed generated/result.md."
      }]
    };
    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnStarted",
            threadId: "thread-1",
            turnId: "turn-remove",
            selectedSkills: []
          }
        });
      }
      await Promise.resolve();
    });
    await emitCompletedTurn("turn-remove", [removalEntry]);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "workspace/files"
      )).length).toBe(deleteRequestCount + 1);
      expect(screen.queryByRole("button", { name: "Open file generated/result.md" })).toBeNull();
    });
  });

  it("runs HTML immediately and reloads the interactive surface when the document changes", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "site", name: "site", kind: "directory", depth: 0 },
        { path: "site/index.html", name: "index.html", kind: "file", depth: 1 },
        { path: "site/other.html", name: "other.html", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const htmlSource = "<!doctype html><html><body><div id=\"app\"></div><script>document.getElementById(\"app\").textContent = \"rendered\"</script></body></html>";
    const changedHtmlSource = htmlSource.replace("rendered", "changed");
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: htmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: htmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    gatewayMock.workspaceFileReadResults.set("site/other.html", {
      path: "site/other.html",
      content: htmlSource,
      binary: false,
      unreadable: null,
      truncated: false
    });
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    fireEvent.click(within(files).getByRole("treeitem", { name: /index\.html/ }));

    const breadcrumb = await within(files).findByRole("navigation", { name: "File breadcrumb" });
    expect(breadcrumb.textContent).toBe("projectsiteindex.html");
    expect(within(files).queryByText("/tmp/project/site/index.html")).toBeNull();
    await waitFor(() => expect(files.querySelector(".htmlStaticPreview iframe")).toBeTruthy());
    const inlineFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null;
    if (!inlineFrame) {
      throw new Error("missing inline HTML preview iframe");
    }
    expect(inlineFrame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-forms");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(inlineFrame.getAttribute("tabindex")).toBe("0");
    expect(inlineFrame.hasAttribute("aria-hidden")).toBe(false);
    expect(inlineFrame.hasAttribute("inert")).toBe(false);
    const inlineDocument = inlineFrame.getAttribute("srcdoc") ?? "";
    expect(inlineDocument).toContain("base-uri 'none'");
    expect(inlineDocument).toContain("form-action 'none'");
    expect(inlineDocument).toContain("frame-src 'none'");
    expect(inlineDocument).toContain("object-src 'none'");
    expect(inlineDocument).not.toContain("script-src 'none'");
    expect(inlineDocument).not.toContain("connect-src 'none'");
    expect(inlineDocument).toContain("textContent = \"rendered\"");
    expect(container.querySelectorAll(".htmlStaticPreview iframe")).toHaveLength(1);
    expect(within(files).queryByRole("button", { name: "Run interactive preview" })).toBeNull();
    expect(within(files).queryByRole("button", { name: "Stop interactive preview" })).toBeNull();

    const openPreviewAction = within(files).getByLabelText("Open HTML preview for site/index.html");
    const editAction = within(files).getByLabelText("Edit site/index.html");
    expect(openPreviewAction.parentElement).toBe(editAction.parentElement);
    expect(openPreviewAction.parentElement?.classList.contains("workspaceFileToolbarActions")).toBe(true);
    expect(openPreviewAction.nextElementSibling).toBe(editAction);

    const fileTreeToggle = within(files).getByRole("button", { name: "File tree" });
    expect(fileTreeToggle.getAttribute("aria-pressed")).toBe("true");
    expect(fileTreeToggle.closest("header")).toBe(files.querySelector(".workspaceFileToolbar"));
    fireEvent.click(fileTreeToggle);
    expect(fileTreeToggle.getAttribute("aria-pressed")).toBe("false");
    expect(within(files).queryByRole("complementary", { name: "Workspace file tree" })).toBeNull();
    expect(files.classList.contains("has-fileTree")).toBe(false);
    fireEvent.click(within(breadcrumb).getByRole("button", { name: "project" }));
    expect(within(files).getByRole("complementary", { name: "Workspace file tree" })).toBeTruthy();
    expect(files.classList.contains("has-fileTree")).toBe(true);
    await waitFor(() => {
      expect(document.activeElement).toBe(within(files).getByLabelText("Filter workspace files"));
    });

    fireEvent.click(within(files).getByLabelText("Edit site/index.html"));
    fireEvent.change(within(files).getByLabelText("Edit site/index.html"), { target: { value: changedHtmlSource } });
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: changedHtmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "written",
      sizeBytes: changedHtmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    fireEvent.click(within(files).getByLabelText("Save file"));
    await waitFor(() => expect(within(files).queryByText("unsaved")).toBeNull());
    await waitFor(() => expect(
      (files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null)?.getAttribute("srcdoc")
    ).toContain("textContent = \"changed\""));
    let activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(activeFrame.getAttribute("srcdoc")).toContain("textContent = \"changed\"");
    fireEvent.click(within(files).getByRole("treeitem", { name: /other\.html/ }));
    await waitFor(() => expect((files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null)?.title).toBe("site/other.html"));
    activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).toBe("allow-scripts");
    fireEvent.click(within(files).getByRole("treeitem", { name: /index\.html/ }));
    await waitFor(() => expect((files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null)?.title).toBe("site/index.html"));
    activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).toBe("allow-scripts");

    fireEvent.click(within(files).getByLabelText("Open HTML preview for site/index.html"));
    const preview = await screen.findByRole("region", { name: "Preview" });
    expect(within(preview).getByRole("heading", { name: "index.html" })).toBeTruthy();
    const previewFrame = within(preview).getByTitle("index.html") as HTMLIFrameElement;
    expect(previewFrame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-forms");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(previewFrame.getAttribute("srcdoc")).toContain("textContent = \"changed\"");
    expect(container.querySelectorAll(".htmlStaticPreview iframe")).toHaveLength(1);
    expect(files.querySelector(".htmlStaticPreview iframe")).toBeNull();
    expect(within(preview).queryByRole("button", { name: "Run interactive preview" })).toBeNull();
    expect(within(preview).queryByRole("button", { name: "Stop interactive preview" })).toBeNull();
  });

  it("saves text edits manually without entering the Review queue", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: "before\n",
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: 7,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    fireEvent.click(await within(files).findByLabelText("Edit docs/README.md"));
    const editor = within(files).getByLabelText("Edit docs/README.md");
    fireEvent.change(editor, { target: { value: "after\n" } });
    fireEvent.click(within(files).getByLabelText("Save file"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/file/write",
        params: expect.objectContaining({
          path: "docs/README.md",
          content: "after\n",
          expectedRevision: "r1",
          force: false
        })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method.startsWith("workspace/change/"))).toBe(false);
  });

  it("renders code previews with absolute paths, syntax tokens, and escaped source text", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.py", name: "main.py", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("src/main.py", {
      path: "src/main.py",
      content: "def greet():\n    return \"<script>alert(1)</script>\"\n",
      binary: false,
      unreadable: null,
      truncated: false
    });
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });

    fireEvent.click(within(files).getByRole("treeitem", { name: /main\.py/ }));
    const breadcrumb = await within(files).findByRole("navigation", { name: "File breadcrumb" });
    expect(breadcrumb.textContent).toBe("projectsrcmain.py");
    expect(within(files).queryByText("/tmp/project/src/main.py")).toBeNull();
    await waitFor(() => expect(container.querySelector(".rightCodePreview")).toBeTruthy());
    const preview = container.querySelector(".rightCodePreview") as HTMLElement | null;
    expect(preview?.dataset.lang).toBe("python");
    expect(preview?.querySelector(".hljs-keyword, .hljs-title")).toBeTruthy();
    expect(preview?.querySelector("script")).toBeNull();
    expect(preview?.innerHTML).toContain("&lt;script&gt;");
  });

  it("keeps Terminal interactive without the persistent title and state header", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Terminal" }));
    const terminal = await screen.findByRole("region", { name: "Terminal" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });

    expect(within(terminal).queryByRole("heading", { name: "project" })).toBeNull();
    expect(within(terminal).queryByText("/tmp/project")).toBeNull();
    expect(within(terminal).queryByText("running")).toBeNull();
  });

  it("uses a readable light xterm theme for Terminal tabs", async () => {
    gatewayMock.xtermTerminalOptions.length = 0;
    window.localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({
      appearance: "light",
      appearanceVersion: 1,
      debug: false,
      rightWidthPx: 520
    }));
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Terminal" }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });

    const theme = gatewayMock.xtermTerminalOptions.at(-1)?.theme as Record<string, string> | undefined;
    expect(theme).toBeTruthy();
    expect(theme?.background).toBe("#f7f5ef");
    expect(theme?.foreground).toBe("#202225");
    expect(theme?.cursor).toBe("#202225");
    expect(theme?.selectionBackground).toBe("#d8dde5");
    expect(theme?.black).toBe("#202225");
    expect(theme?.white).toBe("#5f6670");
    expect(theme?.brightBlack).toBe("#6a6f78");
    expect(theme?.brightWhite).toBe("#3a3f46");
  });
});

function assistantTextEntry(body: string): TranscriptEntry {
  return {
    id: "artifact-links-entry",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "runtime.message",
    blocks: [
      {
        id: "artifact-links-block",
        kind: "text",
        status: "completed",
        order: 0,
        source: "runtime.message",
        title: null,
        body,
        preview: null,
        detail: null,
        artifactIds: [],
        metadata: null,
        result: null,
        createdAtMs: 1,
        updatedAtMs: 1
      }
    ],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1
  };
}

function assistantToolEntry(toolName: "read" | "edit" | "write", path: string): TranscriptEntry {
  const entry = assistantTextEntry("");
  return {
    ...entry,
    id: `${toolName}-entry`,
    blocks: [{
      ...entry.blocks[0]!,
      id: `${toolName}-block`,
      kind: "file",
      title: toolName,
      body: null,
      metadata: {
        projection: "tool",
        tool_name: toolName,
        tool_call_id: `call-${toolName}`,
        args: { path }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: "{}",
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    }]
  };
}

function userTextEntry(body: string): TranscriptEntry {
  return {
    ...assistantTextEntry(body),
    id: "message:1",
    messageSeq: 1,
    role: "user",
    blocks: [
      {
        ...assistantTextEntry(body).blocks[0]!,
        id: "message:1:block",
        body
      }
    ]
  };
}

function userTextEntryForTurn(body: string, turnId: string): TranscriptEntry {
  const entry = userTextEntry(body);
  return {
    ...entry,
    turnId,
    blocks: entry.blocks.map((block) => ({ ...block, body, detail: body, preview: body }))
  };
}

function nativeHistoryEditingContext(historyActionsEnabled: boolean): Record<string, unknown> {
  return {
    runtimeProfileRef: "native",
    selectionState: "default",
    profiles: gatewayMock.runtimeProfileRecords,
    binding: null,
    controls: [],
    stability: "stable",
    capabilities: [{
      id: "turn.start",
      enabled: true,
      stability: "stable",
      unavailableReason: null
    }],
    compatibleTargets: [{
      targetId: "target:default:native",
      agentRef: null,
      runtimeProfileRef: "native",
      agentLabel: "Psychevo",
      profileLabel: "Psychevo (Native)",
      label: "Psychevo · Psychevo (Native)",
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: [
      { kind: "text", enabled: true, unavailableReason: null },
      { kind: "agentMention", enabled: true, unavailableReason: null }
    ],
    actions: [
      {
        id: "forkBefore",
        label: "Fork before message",
        enabled: historyActionsEnabled,
        stability: "stable",
        channelSafe: false,
        unavailableReason: historyActionsEnabled ? null : "A running Thread cannot be forked."
      },
      {
        id: "revertConversation",
        label: "Edit message",
        enabled: historyActionsEnabled,
        stability: "stable",
        channelSafe: false,
        unavailableReason: historyActionsEnabled ? null : "A running Thread cannot be edited."
      }
    ],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: historyActionsEnabled ? "context-idle" : "context-running",
    controlRevision: "controls-native"
  };
}

async function emitGatewayEvent(params: Record<string, unknown>) {
  await act(async () => {
    for (const subscriber of gatewayMock.subscribers) {
      subscriber({ method: "gateway/event", params });
    }
    await Promise.resolve();
  });
}

function completedTurnEvent(turnId: string, entry: TranscriptEntry): Record<string, unknown> {
  return {
    type: "turnCompleted",
    threadId: "thread-1",
    turnId,
    turn: {
      id: turnId,
      threadId: "thread-1",
      status: "completed",
      outcome: "normal",
      error: null,
      startedAtMs: 1,
      completedAtMs: 2
    },
    committedEntries: [entry]
  };
}
