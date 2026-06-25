// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { App } from "./App";

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
      expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/start")).toBe(true);
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
        workdir: "/tmp/project",
        project: { workdir: "/tmp/project", label: "project", displayPath: "/tmp/project" },
        sessions: [projectThread],
        hiddenCount: 0,
        nextCursor: null
      },
      {
        workdir: "/tmp/ops",
        project: { workdir: "/tmp/ops", label: "ops", displayPath: "/tmp/ops" },
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
          scope: expect.objectContaining({ workdir: "/tmp/ops" }),
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
          scope: expect.objectContaining({ workdir: "/tmp/project" })
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
          scope: expect.objectContaining({ workdir: "/tmp/project" })
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

    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(within(page).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "automation/delete",
        params: { automationId: "automation-1" }
      });
    });
    confirm.mockRestore();
    await waitFor(() => {
      expect(within(page).queryByText("Morning check")).toBeNull();
    });
  });

  it("shows last run time and pauses or resumes an existing automation", async () => {
    gatewayMock.automationRecords = [
      {
        id: "automation-1",
        workdir: "/tmp/project",
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
