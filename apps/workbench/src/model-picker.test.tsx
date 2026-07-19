// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelOptionView, ThreadControlDescriptorView } from "@psychevo/protocol";
import { ComposerSubmitControls } from "./composer-controls";
import { ModelReasoningSelector } from "./model-picker";

afterEach(cleanup);

const options: ModelOptionView[] = [{
  provider: "fixture",
  id: "default",
  value: "fixture/default",
  name: "Fixture default",
  providerName: "Fixture",
  free: false,
  limit: { context: null, output: null },
  reasoningSupported: true,
  reasoningEfforts: ["none", "high"]
}];

function renderPicker(
  reasoningPresentation: "selectable" | "readOnly" | "hidden",
  variant: string | null = "high"
) {
  return render(
    <ModelReasoningSelector
      model="fixture/default"
      options={options}
      reasoningPresentation={reasoningPresentation}
      reasoningValues={["none", "high"]}
      showChevron={false}
      variant={variant}
      onModelChange={vi.fn()}
      onVariantChange={vi.fn()}
    />
  );
}

describe("ModelReasoningSelector capability presentation", () => {
  it("keeps selectable reasoning in the grouped picker", () => {
    const { container } = renderPicker("selectable");
    expect(screen.getByRole("button", { name: "Model" }).textContent).toContain("Fixture default High");
    expect(container.querySelector(".lucide-chevron-down")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    const reasoning = screen.getByRole("radiogroup", { name: "Reasoning" });
    expect(within(reasoning).getByRole("radio", { name: "High" }).getAttribute("aria-checked")).toBe("true");
  });

  it("renders an authoritative read-only reasoning value without radio controls", () => {
    renderPicker("readOnly");
    fireEvent.click(screen.getByRole("button", { name: "Model" }));

    expect(screen.getByLabelText("Reasoning: High (read-only)")).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Reasoning" })).toBeNull();
  });

  it("does not invent a reasoning default when the authoritative value is missing", () => {
    renderPicker("selectable", null);
    expect(screen.getByRole("button", { name: "Model" }).textContent).toContain("Unavailable");

    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    const reasoning = screen.getByRole("radiogroup", { name: "Reasoning" });
    expect(within(reasoning).getAllByRole("radio").every((radio) => radio.getAttribute("aria-checked") !== "true")).toBe(true);
  });

  it("renders Default only when the authoritative reasoning value is none", () => {
    renderPicker("selectable", "none");
    expect(screen.getByRole("button", { name: "Model" }).textContent).toContain("Fixture default Default");
  });

  it("omits reasoning when the selected Agent does not expose it", () => {
    renderPicker("hidden");
    expect(screen.getByRole("button", { name: "Model" }).textContent).toBe("Fixture default");

    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    const popover = screen.getByRole("dialog", { name: "Model and reasoning" });
    expect(within(popover).queryByText("Reasoning")).toBeNull();
    expect(within(popover).queryByText("Default")).toBeNull();
  });

  it("uses configured model names for a read-only composer control", () => {
    const modelControl: ThreadControlDescriptorView = {
      id: "model",
      label: "Model",
      surfaceRole: "model",
      mutability: "readOnly",
      enabled: true,
      required: true,
      unavailableReason: null,
      effectiveValue: "fixture/default",
      effectiveSource: "runtimeObserved",
      isDefault: false,
      choices: [{ value: "fixture/default", label: "fixture/default", description: null }],
      applyScope: "session",
      stability: "stable",
      channelSafe: false,
      capabilityRevision: "1",
      dependsOn: null
    };

    render(
      <ComposerSubmitControls
        context={null}
        controls={{
          permissionMode: "default",
          mode: "default",
          runtimeRef: "native",
          agent: null,
          model: "fixture/default",
          modelStatus: "resolved",
          modelError: null,
          variant: null,
          permissionModeOptions: [],
          modeOptions: [],
          modelOptions: ["fixture/default"],
          modelDetails: options,
          recentModels: [],
          variantOptions: []
        }}
        controlValues={{}}
        disabled={false}
        modelControl={modelControl}
        reasoningControl={null}
        usage={null}
        onControlChange={vi.fn()}
      />
    );

    expect(screen.getByLabelText("Model: Fixture default (read-only)").textContent).toBe("Fixture default");
    expect(screen.queryByText("fixture/default")).toBeNull();
  });

  it("uses the authoritative control label while model metadata is unavailable", () => {
    const modelControl: ThreadControlDescriptorView = {
      id: "model",
      label: "Model",
      surfaceRole: "model",
      mutability: "readOnly",
      enabled: true,
      required: true,
      unavailableReason: null,
      effectiveValue: "fixture/default",
      effectiveSource: "runtimeObserved",
      isDefault: false,
      choices: [{ value: "fixture/default", label: "Fixture default", description: null }],
      applyScope: "session",
      stability: "stable",
      channelSafe: false,
      capabilityRevision: "1",
      dependsOn: null
    };

    render(
      <ComposerSubmitControls
        context={null}
        controls={{
          permissionMode: "default",
          mode: "default",
          runtimeRef: "native",
          agent: null,
          model: "fixture/default",
          modelStatus: "resolved",
          modelError: null,
          variant: null,
          permissionModeOptions: [],
          modeOptions: [],
          modelOptions: ["fixture/default"],
          modelDetails: [],
          recentModels: [],
          variantOptions: []
        }}
        controlValues={{}}
        disabled={false}
        modelControl={modelControl}
        reasoningControl={null}
        usage={null}
        onControlChange={vi.fn()}
      />
    );

    expect(screen.getByLabelText("Model: Fixture default (read-only)").textContent).toBe("Fixture default");
  });
});
