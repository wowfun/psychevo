// @vitest-environment jsdom

import {
  StrictMode,
  createElement,
  useEffect,
  useMemo,
  useSyncExternalStore
} from "react";
import { act, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ThreadSessionView } from "@psychevo/client";
import type { ThreadContextReadResult, ThreadSnapshot } from "@psychevo/protocol";
import {
  ThreadSessionOperationalApplication,
  type ThreadSessionViewSource
} from "./thread-session-operational-application";

describe("ThreadSessionOperationalApplication", () => {
  it("reattaches after the Strict Effects cleanup probe", () => {
    const source = new FakeThreadSessionViewSource();

    function Root() {
      const application = useMemo(
        () => new ThreadSessionOperationalApplication(source),
        []
      );
      const operational = useSyncExternalStore(
        application.subscribe,
        application.getSnapshot,
        application.getSnapshot
      );
      useEffect(() => () => application.dispose(), [application]);
      return createElement("span", null, operational.threadSnapshot ? "loaded" : "empty");
    }

    const mounted = render(createElement(StrictMode, null, createElement(Root)));
    const threadSnapshot = { id: "snapshot-after-probe" } as unknown as ThreadSnapshot;
    act(() => source.publish({ ...source.getView(), threadSnapshot }));

    expect(mounted.getByText("loaded")).toBeTruthy();
  });

  it("does not publish when only the live transcript overlay changes", () => {
    const source = new FakeThreadSessionViewSource();
    const application = new ThreadSessionOperationalApplication(source);
    const listener = vi.fn();
    application.subscribe(listener);
    const before = application.getSnapshot();

    source.publish({ ...source.getView(), liveEntries: [liveEntry()] });

    expect(listener).not.toHaveBeenCalled();
    expect(application.getSnapshot()).toBe(before);
  });

  it("publishes committed snapshot and context changes, then detaches on dispose", () => {
    const source = new FakeThreadSessionViewSource();
    const application = new ThreadSessionOperationalApplication(source);
    const listener = vi.fn();
    application.subscribe(listener);

    const threadSnapshot = { id: "snapshot-2" } as unknown as ThreadSnapshot;
    source.publish({ ...source.getView(), threadSnapshot });
    expect(listener).toHaveBeenCalledOnce();
    expect(application.getSnapshot().threadSnapshot).toBe(threadSnapshot);

    const context = { controlRevision: "context-2" } as ThreadContextReadResult;
    source.publish({ ...source.getView(), context });
    expect(listener).toHaveBeenCalledTimes(2);
    expect(application.getSnapshot().context).toBe(context);

    application.dispose();
    source.publish({ ...source.getView(), threadSnapshot: null });
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

class FakeThreadSessionViewSource implements ThreadSessionViewSource {
  private readonly listeners = new Set<() => void>();
  private view: ThreadSessionView = {
    context: null,
    liveEntries: [],
    threadSnapshot: null
  };

  getView(): ThreadSessionView {
    return this.view;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  publish(view: ThreadSessionView): void {
    this.view = view;
    for (const listener of this.listeners) listener();
  }
}

function liveEntry(): ThreadSessionView["liveEntries"][number] {
  return {
    blocks: [],
    createdAtMs: 1,
    id: "live-entry",
    role: "assistant",
    source: "runtime",
    status: "running",
    threadId: "thread-1",
    turnId: "turn-1",
    updatedAtMs: 1
  };
}
