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
    expect(screen.queryByPlaceholderText("Ask Psychevo...")).toBeNull();
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
