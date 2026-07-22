// @vitest-environment jsdom

import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { deferred, gatewayMock } from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench journey diagnostics", () => {
  it("exposes connected, draft readiness, and turn activity as stable shell state", async () => {
    const draftOpen = deferred<Record<string, unknown>>();
    gatewayMock.draftOpen = () => draftOpen.promise;

    render(<App />);

    const shell = shellElement();
    await waitFor(() => expect(shell.dataset.gatewayStatus).toBe("connected"));
    const preparingInput = await screen.findByPlaceholderText("Preparing runtime environment…") as HTMLTextAreaElement;
    expect(preparingInput.disabled).toBe(true);
    expect(document.querySelector(".composerDock")?.getAttribute("aria-busy")).toBe("true");
    expect(window.__psychevoJourneyTiming?.["psychevo:gui_ready"]).toBeUndefined();
    expect(shell.dataset.composerState).toBe("opening");
    expect(shell.dataset.turnState).toBe("idle");

    await act(async () => {
      draftOpen.resolve(draftOpenResult());
      await draftOpen.promise;
    });
    await screen.findByPlaceholderText("Ask Psychevo...");
    await waitFor(() => expect(shell.dataset.composerState).toBe("ready"));
    await waitFor(() => expect(
      window.__psychevoJourneyTiming?.["psychevo:gui_ready"]?.monotonicMs
    ).toEqual(expect.any(Number)));
    await waitFor(() => expect(
      window.__psychevoJourneyTiming?.["psychevo:draft_context_ready"]?.monotonicMs
    ).toEqual(expect.any(Number)));

    act(() => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "activityChanged",
            threadId: null,
            activity: { running: true, activeTurnId: "turn-1", queuedTurns: 0 }
          }
        });
      }
    });
    await waitFor(() => expect(shell.dataset.turnState).toBe("running"));
  });

  it("keeps a blocked shell recoverable without accepting stale startup results", async () => {
    let attempts = 0;
    gatewayMock.draftOpen = () => {
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new Error("runtime preparation failed"))
        : draftOpenResult();
    };

    render(<App />);

    await screen.findByPlaceholderText("Preparing runtime environment…");
    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(shellElement().dataset.composerState).toBe("blocked");
    const alerts = screen.getAllByRole("alert");
    expect(alerts).toHaveLength(1);
    expect(alerts[0]?.textContent).toContain("runtime preparation failed");
    expect(document.querySelector(".errorBand")).toBeNull();

    await act(async () => {
      retry.click();
    });

    await waitFor(() => {
      expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
      expect(shellElement().dataset.composerState).toBe("ready");
    });
    expect(attempts).toBe(2);
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "turn/start")).toHaveLength(0);
  });

  it("keeps a retry preparation failure in the single Composer alert", async () => {
    let attempts = 0;
    gatewayMock.draftOpen = () => {
      attempts += 1;
      if (attempts === 1) {
        return {
          ...draftOpenResult(),
          problem: { message: "initial preparation failed" }
        };
      }
      return Promise.reject(new Error("retry preparation failed"));
    };

    render(<App />);

    const retry = await screen.findByRole("button", { name: "Retry" });
    await act(async () => {
      retry.click();
    });

    await waitFor(() => {
      const alerts = screen.getAllByRole("alert");
      expect(alerts).toHaveLength(1);
      expect(alerts[0]?.textContent).toContain("retry preparation failed");
      expect(document.querySelector(".errorBand")).toBeNull();
    });
    expect(attempts).toBe(2);
  });
});

function shellElement(): HTMLElement {
  const shell = document.querySelector(".appShell");
  if (!(shell instanceof HTMLElement)) throw new Error("Workbench shell is missing");
  return shell;
}

function draftOpenResult(): Record<string, unknown> {
  return {
    snapshot: {
      ...gatewayMock.snapshot,
      thread: null,
      entries: [],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    },
    context: {
      selectedTargetId: "target:default:native",
      suggestedTargetId: null,
      runtimeProfileRef: "native",
      selectionState: "draft",
      profiles: [],
      binding: null,
      controls: [],
      stability: "stable",
      capabilities: [],
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
      inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
      actions: [],
      sendability: { allowed: true, reason: null, recoveryAction: null },
      history: { owner: "psychevo", fidelity: "unavailable", cursor: null, hint: null },
      pendingInteractions: [],
      contextRevision: "context-native",
      controlRevision: "controls-native"
    },
    problem: null
  };
}
