import { Mic, Radio, Volume2 } from "lucide-react";
import { IconButton, Switch } from "@psychevo/components";

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
    <IconButton
      aria-pressed={listening}
      className={`composerDictationButton ${listening ? "is-listening" : ""}`.trim()}
      disabled={disabled}
      icon={(
        <>
          <span className="composerDictationPulse" aria-hidden />
          <Mic size={16} aria-hidden />
        </>
      )}
      label={listening ? "Stop dictation" : "Start dictation"}
      onClick={onToggle}
      shape="circle"
    />
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
        icon={<Volume2 size={14} />}
        label="Auto-speak"
        onCheckedChange={onToggleAutoSpeak}
        size="compact"
      />
      <Switch
        checked={realtimeActive}
        className="pevo-modeSwitchRow composerVoiceOptionRow"
        disabled={disabled}
        icon={<Radio size={14} />}
        label="Realtime voice"
        onCheckedChange={onToggleRealtime}
        size="compact"
      />
    </>
  );
}
