// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ComposerDictationButton, ComposerVoiceOptionSwitches } from "./voice-controls";

describe("ComposerDictationButton", () => {
  it("routes dictation and switches labels while listening", () => {
    const onToggle = vi.fn();

    const { rerender } = render(
      <ComposerDictationButton
        disabled={false}
        listening={false}
        onToggle={onToggle}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    expect(onToggle).toHaveBeenCalledTimes(1);

    rerender(
      <ComposerDictationButton
        disabled={false}
        listening
        onToggle={onToggle}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Stop dictation" }));
    expect(onToggle).toHaveBeenCalledTimes(2);
  });
});

describe("ComposerVoiceOptionSwitches", () => {
  it("renders auto-speak and realtime as icon-led labelled switches", () => {
    const onToggleAutoSpeak = vi.fn();
    const onToggleRealtime = vi.fn();

    const { container } = render(
      <ComposerVoiceOptionSwitches
        autoSpeak={false}
        disabled={false}
        realtimeActive={false}
        onToggleAutoSpeak={onToggleAutoSpeak}
        onToggleRealtime={onToggleRealtime}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "Auto-speak" }));
    fireEvent.click(screen.getByRole("switch", { name: "Realtime voice" }));

    expect(onToggleAutoSpeak).toHaveBeenCalledTimes(1);
    expect(onToggleRealtime).toHaveBeenCalledTimes(1);
    expect(container.querySelector(".lucide-volume-2")).toBeTruthy();
    expect(container.querySelector(".lucide-radio")).toBeTruthy();
  });
});
