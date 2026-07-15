// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SessionSummary } from "@psychevo/protocol";
import { DeleteSessionDialog } from "./delete-session-dialog";

afterEach(cleanup);

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
