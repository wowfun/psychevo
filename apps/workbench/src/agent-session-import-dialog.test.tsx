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

  it("installs unavailable Codex ACP profiles and refreshes discovery", async () => {
    let listCalls = 0;
    const request = vi.fn(async (method: string) => {
      if (method === "thread/import/list") {
        listCalls += 1;
        return listCalls === 1 ? {
          profiles: [{
            runtimeProfileRef: "codex",
            profileLabel: "Codex (ACP)",
            targets: [],
            status: "error",
            sessions: [],
            nextCursor: null,
            alreadyImportedCount: 0,
            error: {
              message: "Managed Codex ACP is not installed.",
              code: "acp_backend_unavailable",
              stage: "configuration",
              retryClass: "user_action",
              delivery: "notDelivered",
              recoveryAction: "backend/install",
              diagnosticRef: null
            }
          }]
        } : {
          profiles: [{
            runtimeProfileRef: "codex",
            profileLabel: "Codex (ACP)",
            targets: [],
            status: "ready",
            sessions: [],
            nextCursor: null,
            alreadyImportedCount: 0,
            error: null
          }]
        };
      }
      if (method === "backend/install") {
        return {
          id: "codex",
          operation: "install",
          changed: true,
          status: "ready",
          path: "/tmp/psychevo/runtime-adapters/codex-acp/1.1.2",
          message: "Managed Codex ACP 1.1.2 is ready."
        };
      }
      throw new Error(`unexpected request: ${method}`);
    });

    render(
      <AgentSessionImportDialog
        client={{ request } as unknown as GatewayClient}
        disabled={false}
        onClose={vi.fn()}
        onImported={vi.fn()}
        scope={scope}
      />
    );

    expect(await screen.findByRole("button", { name: "Install Codex ACP" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Install Codex ACP" }));
    await waitFor(() => expect(request).toHaveBeenCalledWith("backend/install", { id: "codex", scope }));
    await waitFor(() => expect(listCalls).toBe(2));
    expect(screen.queryByRole("button", { name: "Install Codex ACP" })).toBeNull();
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
