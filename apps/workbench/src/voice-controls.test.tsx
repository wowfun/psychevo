// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ComposerVoiceControls } from "./voice-controls";

describe("ComposerVoiceControls", () => {
  it("routes dictation, auto-speak, and realtime toggles", () => {
    const onToggleAutoSpeak = vi.fn();
    const onToggleDictation = vi.fn();
    const onToggleRealtime = vi.fn();

    render(
      <ComposerVoiceControls
        autoSpeak={false}
        disabled={false}
        listening={false}
        realtimeActive={false}
        onToggleAutoSpeak={onToggleAutoSpeak}
        onToggleDictation={onToggleDictation}
        onToggleRealtime={onToggleRealtime}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    fireEvent.click(screen.getByRole("button", { name: "Enable auto-speak" }));
    fireEvent.click(screen.getByRole("button", { name: "Start realtime voice" }));

    expect(onToggleDictation).toHaveBeenCalledTimes(1);
    expect(onToggleAutoSpeak).toHaveBeenCalledTimes(1);
    expect(onToggleRealtime).toHaveBeenCalledTimes(1);
  });
});
