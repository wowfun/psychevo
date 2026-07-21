// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { GatewayClient } from "@psychevo/client";
import { App } from "./App";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Workbench automations", () => {
  it("shows one empty-state creation surface before a draft is opened", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "automation/list")).toBe(true);
    });

    expect(within(page).getAllByRole("button", { name: "Project check" })).toHaveLength(1);
    expect(within(page).getAllByRole("button", { name: "Thread heartbeat" })).toHaveLength(1);
    expect(within(page).queryByRole("form", { name: "Automation draft" })).toBeNull();
    expect(page.querySelector(".automationDraftPlaceholder")).toBeNull();
  });

  it("returns to the transcript when starting a new session from Automations", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    expect(await screen.findByRole("region", { name: "Automations" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/draft/open")).toBe(true);
    });
    expect(await screen.findByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Automations" })).toBeNull();
  });

  it("saves a draft against the selected workspace thread", async () => {
    const projectThread = sessionSummary("thread-1", "Project session", "/tmp/project");
    const opsThread = sessionSummary("thread-ops", "Ops release check", "/tmp/ops");
    gatewayMock.sessionSummaries = [projectThread, opsThread];
    gatewayMock.browserWorkspaces = [
      {
        cwd: "/tmp/project",
        project: { cwd: "/tmp/project", label: "project", displayPath: "/tmp/project" },
        sessions: [projectThread],
        hiddenCount: 0,
        nextCursor: null
      },
      {
        cwd: "/tmp/ops",
        project: { cwd: "/tmp/ops", label: "ops", displayPath: "/tmp/ops" },
        sessions: [opsThread],
        hiddenCount: 0,
        nextCursor: null
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    await waitFor(() => {
      expect(within(page).getByLabelText("Workspace")).toBeTruthy();
    });

    fireEvent.change(within(page).getByLabelText("Workspace"), { target: { value: "/tmp/ops" } });
    fireEvent.click(within(page).getByRole("button", { name: "Thread heartbeat" }));
    fireEvent.change(within(page).getByLabelText("Bind to"), { target: { value: "thread-ops" } });
    fireEvent.change(within(page).getByLabelText("Title"), { target: { value: "Ops heartbeat" } });
    fireEvent.change(within(page).getByLabelText("Prompt"), { target: { value: "Continue the release check." } });
    fireEvent.click(within(page).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/write",
        params: expect.objectContaining({
          scope: expect.objectContaining({ cwd: "/tmp/ops" }),
          target: { kind: "threadHeartbeat", threadId: "thread-ops" },
          title: "Ops heartbeat"
        })
      });
    });
  });

  it("drafts a project automation from natural language before saving", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    const request = "Every weekday at 9, review the repo before standup.";

    fireEvent.change(within(page).getByLabelText("Automation description"), {
      target: { value: request }
    });
    fireEvent.click(within(page).getByRole("button", { name: "Draft" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/draft",
        params: expect.objectContaining({
          request,
          currentThreadId: null,
          scope: expect.objectContaining({ cwd: "/tmp/project" })
        })
      });
    });

    const title = within(page).getByLabelText("Title") as HTMLInputElement;
    const prompt = within(page).getByLabelText("Prompt") as HTMLTextAreaElement;
    expect(title.value).toBe("Morning repository check");
    expect(prompt.value).toContain("before standup");

    fireEvent.click(within(page).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/write",
        params: expect.objectContaining({
          title: "Morning repository check",
          prompt: "Review current repository state and summarize risky work before standup.",
          schedule: { kind: "daily", time: "09:00" }
        })
      });
    });
  });

  it("creates, runs, and deletes a project automation", async () => {
    let releaseDelete!: () => void;
    const deleteGate = new Promise<void>((resolve) => {
      releaseDelete = resolve;
    });
    const originalRequest = GatewayClient.prototype.request;
    const requestSpy = vi.spyOn(GatewayClient.prototype, "request").mockImplementation(function (this: GatewayClient, method, params) {
      const result = Reflect.apply(originalRequest, this, [method, params]) as ReturnType<GatewayClient["request"]>;
      if (method !== "automation/delete") return result;
      return result.then(async (value) => {
        await deleteGate;
        return value;
      }) as ReturnType<GatewayClient["request"]>;
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "automation/list")).toBe(true);
    });

    fireEvent.click(within(page).getByRole("button", { name: "Project check" }));
    fireEvent.change(within(page).getByLabelText("Title"), { target: { value: "Morning check" } });
    fireEvent.change(within(page).getByLabelText("Prompt"), { target: { value: "Review open risks before standup." } });
    fireEvent.click(within(page).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/write",
        params: expect.objectContaining({
          title: "Morning check",
          prompt: "Review open risks before standup.",
          target: { kind: "project" },
          schedule: { kind: "interval", everyMinutes: 60 },
          execution: { policy: "autoSandbox" },
          scope: expect.objectContaining({ cwd: "/tmp/project" })
        })
      });
    });
    const writeRequest = gatewayMock.requestLog.find((entry) => entry.method === "automation/write");
    expect(Object.prototype.hasOwnProperty.call(writeRequest?.params ?? {}, "enabled")).toBe(false);
    expect(await within(page).findByText("Morning check")).toBeTruthy();

    fireEvent.click(within(page).getByRole("button", { name: "Run" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/run",
        params: { automationId: "automation-1", trigger: "manual" }
      });
    });
    expect(await within(page).findByText("running")).toBeTruthy();

    fireEvent.click(within(page).getByRole("button", { name: "Delete" }));
    let deleteDialog = await screen.findByRole("dialog", { name: "Delete Morning check?" });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "automation/delete")).toBe(false);
    fireEvent.click(within(deleteDialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Delete Morning check?" })).toBeNull();
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "automation/delete")).toBe(false);

    fireEvent.click(within(page).getByRole("button", { name: "Delete" }));
    deleteDialog = await screen.findByRole("dialog", { name: "Delete Morning check?" });
    fireEvent.click(within(deleteDialog).getByRole("button", { name: "Delete automation" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/delete",
        params: { automationId: "automation-1" }
      });
      expect(requestSpy).toHaveBeenCalledWith("automation/delete", { automationId: "automation-1" });
    });
    await waitFor(() => {
      const pendingDialog = screen.getByRole("dialog", { name: "Delete Morning check?" });
      expect(pendingDialog.getAttribute("aria-busy")).toBe("true");
      expect((within(pendingDialog).getByRole("button", { name: "Close" }) as HTMLButtonElement).disabled).toBe(true);
      expect((within(pendingDialog).getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true);
      expect((within(pendingDialog).getByRole("button", { name: "Delete automation" }) as HTMLButtonElement).disabled).toBe(true);
    });
    expect(within(page).getByText("Morning check")).toBeTruthy();

    releaseDelete();
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Delete Morning check?" })).toBeNull();
    });
    await waitFor(() => {
      expect(within(page).queryByText("Morning check")).toBeNull();
    });
  });

  it("shows last run time and pauses or resumes an existing automation", async () => {
    gatewayMock.automationRecords = [
      {
        id: "automation-1",
        cwd: "/tmp/project",
        kind: "project",
        targetThreadId: null,
        title: "Morning check",
        prompt: "Review open risks before standup.",
        schedule: { kind: "interval", everyMinutes: 60 },
        enabled: true,
        execution: { policy: "autoSandbox" },
        model: null,
        reasoningEffort: null,
        sourceKey: "automation:automation-1",
        createdAtMs: Date.UTC(2026, 5, 24, 8, 0),
        updatedAtMs: Date.UTC(2026, 5, 24, 8, 0),
        lastRunAtMs: Date.UTC(2026, 5, 24, 9, 30),
        nextRunAtMs: Date.UTC(2026, 5, 24, 10, 30),
        lastStatus: "completed",
        lastError: null,
        runs: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    expect(await within(page).findByText("Morning check")).toBeTruthy();
    expect(within(page).getByText(/last run/i)).toBeTruthy();

    fireEvent.click(within(page).getByRole("button", { name: "Pause" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/pause",
        params: { automationId: "automation-1" }
      });
    });
    expect(await within(page).findByText("paused")).toBeTruthy();

    fireEvent.click(within(page).getByRole("button", { name: "Resume" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/resume",
        params: { automationId: "automation-1" }
      });
    });
  });

  it("keeps paused lifecycle separate from a running last status", async () => {
    gatewayMock.automationRecords = [
      {
        id: "automation-1",
        cwd: "/tmp/project",
        kind: "project",
        targetThreadId: null,
        title: "Paused running check",
        prompt: "Review stale run state.",
        schedule: { kind: "interval", everyMinutes: 60 },
        enabled: false,
        execution: { policy: "autoSandbox" },
        model: null,
        reasoningEffort: null,
        sourceKey: "automation:automation-1",
        createdAtMs: Date.UTC(2026, 5, 24, 8, 0),
        updatedAtMs: Date.UTC(2026, 5, 24, 8, 0),
        lastRunAtMs: Date.UTC(2026, 5, 24, 9, 30),
        nextRunAtMs: null,
        lastStatus: "running",
        lastError: null,
        runs: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    const row = (await within(page).findByText("Paused running check")).closest(".automationRow");
    expect(row?.getAttribute("data-lifecycle")).toBe("paused");
    expect(row?.querySelector(".automationRowTitle span")?.textContent).toBe("paused");
    expect(row?.querySelector(".automationMeta span[data-run-status=\"running\"]")?.textContent).toBe("running");
  });

  it("opens the newest non-empty run thread when the latest run has no thread", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-old", "Older automation run")];
    gatewayMock.automationRecords = [
      {
        id: "automation-1",
        cwd: "/tmp/project",
        kind: "project",
        targetThreadId: null,
        title: "Project heartbeat",
        prompt: "Continue project work.",
        schedule: { kind: "interval", everyMinutes: 60 },
        enabled: true,
        execution: { policy: "autoSandbox" },
        model: null,
        reasoningEffort: null,
        sourceKey: "automation:automation-1",
        createdAtMs: Date.UTC(2026, 5, 24, 8, 0),
        updatedAtMs: Date.UTC(2026, 5, 24, 8, 0),
        lastRunAtMs: Date.UTC(2026, 5, 24, 9, 30),
        nextRunAtMs: Date.UTC(2026, 5, 24, 10, 30),
        lastStatus: "running",
        lastError: null,
        runs: [
          {
            id: "run-latest",
            automationId: "automation-1",
            trigger: "scheduler",
            status: "running",
            startedAtMs: Date.UTC(2026, 5, 24, 9, 30),
            completedAtMs: null,
            threadId: null,
            sourceKey: null,
            error: null,
            metadata: null
          },
          {
            id: "run-old",
            automationId: "automation-1",
            trigger: "scheduler",
            status: "completed",
            startedAtMs: Date.UTC(2026, 5, 24, 9, 0),
            completedAtMs: Date.UTC(2026, 5, 24, 9, 1),
            threadId: "thread-old",
            sourceKey: "automation:automation-1",
            error: null,
            metadata: null
          }
        ]
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    fireEvent.click(await within(page).findByRole("button", { name: "Open thread" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-old" })
      });
    });
  });

  it("creates a current-thread heartbeat automation", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Active session")];
    render(<App />);

    fireEvent.click(await screen.findByText("Active session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    fireEvent.click(await screen.findByRole("button", { name: "Automations" }));
    const page = await screen.findByRole("region", { name: "Automations" });
    fireEvent.click(within(page).getByRole("button", { name: "Thread heartbeat" }));
    fireEvent.click(within(page).getByRole("button", { name: "Ask first" }));
    fireEvent.click(within(page).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/write",
        params: expect.objectContaining({
          title: "Thread heartbeat",
          target: { kind: "threadHeartbeat", threadId: "thread-1" },
          schedule: { kind: "interval", everyMinutes: 30 },
          execution: { policy: "askFirst" }
        })
      });
    });
    await waitFor(() => {
      expect(within(page).getAllByText("Thread heartbeat").length).toBeGreaterThan(0);
    });
  });
});
