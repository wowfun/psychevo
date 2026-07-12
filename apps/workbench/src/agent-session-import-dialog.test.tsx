// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope, SessionSummary } from "@psychevo/protocol";
import { AgentSessionImportDialog, DeleteSessionDialog } from "./agent-session-import-dialog";

afterEach(cleanup);

const scope: GatewayRequestScope = {
  cwd: "/workspace",
  source: {
    kind: "web",
    rawId: "import-dialog-test",
    lifetime: "persistent",
    rawIdentity: null,
    visibleName: null
  }
};

describe("AgentSessionImportDialog", () => {
  it("discovers only when mounted and imports the selected opaque candidate", async () => {
    const request = vi.fn(async (method: string) => {
      if (method === "thread/import/list") {
        return {
          profiles: [{
            runtimeProfileRef: "opencode",
            profileLabel: "OpenCode",
            targets: [{
              targetId: "target:opaque",
              agentRef: "opencode",
              runtimeProfileRef: "opencode",
              agentLabel: "OpenCode",
              profileLabel: "OpenCode",
              label: "OpenCode",
              ready: true,
              unavailableReason: null
            }],
            status: "ready",
            sessions: [{
              candidateId: "candidate:opaque",
              cwd: "/workspace",
              title: "Imported conversation",
              updatedAt: null
            }],
            nextCursor: null,
            alreadyImportedCount: 0,
            error: null
          }]
        };
      }
      return {
        snapshot: {
          source: { kind: "web", rawId: "import-dialog-test", lifetime: "persistent", rawIdentity: null, visibleName: null },
          scope,
          thread: { id: "thread-imported", backend: { kind: "acp", runtimeRef: "opencode", nativeId: null }, sourceKey: null },
          history: { owner: "agent", fidelity: "full", cursor: null, hint: null },
          entries: [],
          activity: { running: false, activeTurnId: null, queuedTurns: 0 },
          pendingActions: []
        }
      };
    });
    const onImported = vi.fn();
    render(
      <AgentSessionImportDialog
        client={{ request } as unknown as GatewayClient}
        disabled={false}
        onClose={vi.fn()}
        onImported={onImported}
        scope={scope}
      />
    );

    expect(await screen.findByText("Imported conversation")).toBeTruthy();
    expect(request).toHaveBeenCalledWith("thread/import/list", { scope, cursors: {} });
    fireEvent.click(screen.getByRole("button", { name: /Imported conversation/ }));
    await waitFor(() => expect(request).toHaveBeenCalledWith("thread/import", {
      candidateId: "candidate:opaque",
      scope,
      targetId: "target:opaque"
    }));
    expect(onImported).toHaveBeenCalledWith("thread-imported");
  });
});

describe("DeleteSessionDialog", () => {
  it("names the remote Agent deletion before confirmation", () => {
    const session = {
      id: "thread-remote",
      displayTitle: "Remote conversation",
      title: null,
      lifecycle: {
        targetLabel: "Codex",
        actions: [{ id: "delete", enabled: true, unavailableReason: null }]
      }
    } as SessionSummary;
    render(
      <DeleteSessionDialog
        disabled={false}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
        session={session}
      />
    );
    expect(screen.getByText(/Codex session/)).toBeTruthy();
    expect(screen.getByText(/Remote deletion must succeed/)).toBeTruthy();
  });
});
