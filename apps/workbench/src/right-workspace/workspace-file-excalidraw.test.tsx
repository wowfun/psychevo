// @vitest-environment jsdom

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceExcalidrawScene } from "./workspace-file-excalidraw-data";
import { ExcalidrawPreview } from "./workspace-file-excalidraw";

const { renderScene } = vi.hoisted(() => ({ renderScene: vi.fn() }));

vi.mock("./workspace-file-excalidraw-renderer", () => ({
  renderExcalidrawScene: renderScene
}));

afterEach(() => {
  cleanup();
  renderScene.mockReset();
});

describe("ExcalidrawPreview Adapter", () => {
  it("does not start while inactive", async () => {
    render(
      <ExcalidrawPreview
        active={false}
        onStateChange={() => undefined}
        path="inactive.excalidraw"
        scene={scene("inactive")}
      />
    );
    await act(async () => Promise.resolve());
    expect(renderScene).not.toHaveBeenCalled();
  });

  it("never commits stale output and keeps completed DOM while inactive", async () => {
    const first = deferred<SVGSVGElement>();
    renderScene
      .mockImplementationOnce(() => first.promise)
      .mockResolvedValueOnce(svg("Preview second.excalidraw"));
    const onStateChange = vi.fn();
    const firstScene = scene("first");
    const secondScene = scene("second");
    const view = render(
      <ExcalidrawPreview
        active
        onStateChange={onStateChange}
        path="first.excalidraw"
        scene={firstScene}
      />
    );
    await waitFor(() => expect(renderScene).toHaveBeenCalledTimes(1));

    view.rerender(
      <ExcalidrawPreview
        active
        onStateChange={onStateChange}
        path="second.excalidraw"
        scene={secondScene}
      />
    );
    expect(await screen.findByRole("img", { name: "Preview second.excalidraw" })).toBeTruthy();

    await act(async () => {
      first.resolve(svg("Preview first.excalidraw"));
      await first.promise;
    });
    expect(screen.queryByRole("img", { name: "Preview first.excalidraw" })).toBeNull();

    view.rerender(
      <ExcalidrawPreview
        active={false}
        onStateChange={onStateChange}
        path="second.excalidraw"
        scene={secondScene}
      />
    );
    expect(screen.getByRole("img", { name: "Preview second.excalidraw" })).toBeTruthy();
  });
});

function scene(id: string): WorkspaceExcalidrawScene {
  return {
    appState: {
      exportBackground: true,
      exportEmbedScene: false,
      exportWithDarkMode: false,
      theme: "light",
      viewBackgroundColor: "#ffffff"
    },
    elements: [{ id, type: "rectangle" }],
    files: {}
  };
}

function svg(label: string): SVGSVGElement {
  const element = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  element.setAttribute("aria-label", label);
  element.setAttribute("role", "img");
  return element;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
