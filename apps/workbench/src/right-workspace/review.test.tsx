// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ThreadSnapshot, WorkspaceChangesResult } from "@psychevo/protocol";
import { ReviewPanel } from "./review";

afterEach(cleanup);

describe("ReviewPanel workspace invalidations", () => {
  it("shows opaque mutations without offering a fake Reject action", () => {
    const changes: WorkspaceChangesResult = {
      groups: [{
        turnId: "turn-opaque",
        threadId: "thread-1",
        createdAtMs: 1,
        completedAtMs: 2,
        files: [],
        invalidations: [{
          source: "exec_command",
          message: "Workspace may have changed via exec_command; inspect the diff. Exact Reject is unavailable."
        }]
      }]
    };

    render(
      <ReviewPanel
        activity={{ running: false, activeTurnId: null, queuedTurns: 0 } satisfies ThreadSnapshot["activity"]}
        changedFiles={[]}
        changes={changes}
        context={null}
        cwd="/workspace"
        diff={null}
        root="/workspace"
        sessionId="thread-1"
        status="Connected"
        onAcceptChange={vi.fn()}
        onChangedFile={vi.fn()}
        onRefresh={vi.fn()}
        onRejectChange={vi.fn()}
      />
    );

    expect(screen.getByText("exec_command")).toBeTruthy();
    expect(screen.getByText("inspect diff")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Reject exec_command/ })).toBeNull();
  });
});
