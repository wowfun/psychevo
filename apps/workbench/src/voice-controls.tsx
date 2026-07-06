import { Mic } from "lucide-react";
import { Switch } from "@psychevo/components";

export function ComposerDictationButton({
  disabled,
  listening,
  onToggle
}: {
  disabled: boolean;
  listening: boolean;
  onToggle(): void;
}) {
  return (
    <button
      aria-label={listening ? "Stop dictation" : "Start dictation"}
      aria-pressed={listening}
      className={`composerDictationButton ${listening ? "is-listening" : ""}`.trim()}
      disabled={disabled}
      onClick={onToggle}
      title={listening ? "Stop dictation" : "Start dictation"}
      type="button"
    >
      <span className="composerDictationPulse" aria-hidden />
      <Mic size={16} aria-hidden />
    </button>
  );
}

export function ComposerVoiceOptionSwitches({
  autoSpeak,
  disabled,
  realtimeActive,
  onToggleAutoSpeak,
  onToggleRealtime
}: {
  autoSpeak: boolean;
  disabled: boolean;
  realtimeActive: boolean;
  onToggleAutoSpeak(): void;
  onToggleRealtime(): void;
}) {
  return (
    <>
      <Switch
        checked={autoSpeak}
        className="pevo-modeSwitchRow composerVoiceOptionRow"
        disabled={disabled}
        label="Auto-speak"
        onCheckedChange={onToggleAutoSpeak}
        size="compact"
      />
      <Switch
        checked={realtimeActive}
        className="pevo-modeSwitchRow composerVoiceOptionRow"
        disabled={disabled}
        label="Realtime voice"
        onCheckedChange={onToggleRealtime}
        size="compact"
      />
    </>
  );
}
