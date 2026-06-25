// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench automations", () => {
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

    fireEvent.click(within(page).getAllByRole("button", { name: "Project check" })[0]!);
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
    fireEvent.click(within(page).getAllByRole("button", { name: "Thread heartbeat" })[0]!);
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
