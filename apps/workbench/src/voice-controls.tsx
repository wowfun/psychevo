import { Mic, MicOff, Radio, Volume2, VolumeX } from "lucide-react";

export function ComposerVoiceControls({
  autoSpeak,
  disabled,
  listening,
  realtimeActive,
  onToggleAutoSpeak,
  onToggleDictation,
  onToggleRealtime
}: {
  autoSpeak: boolean;
  disabled: boolean;
  listening: boolean;
  realtimeActive: boolean;
  onToggleAutoSpeak(): void;
  onToggleDictation(): void;
  onToggleRealtime(): void;
}) {
  return (
    <div className="composerVoiceControls" aria-label="Voice controls">
      <button
        aria-label={listening ? "Stop dictation" : "Start dictation"}
        aria-pressed={listening}
        className={`composerVoiceButton ${listening ? "is-active" : ""}`.trim()}
        disabled={disabled}
        onClick={onToggleDictation}
        title={listening ? "Stop dictation" : "Start dictation"}
        type="button"
      >
        {listening ? <MicOff size={16} aria-hidden /> : <Mic size={16} aria-hidden />}
      </button>
      <button
        aria-label={autoSpeak ? "Disable auto-speak" : "Enable auto-speak"}
        aria-pressed={autoSpeak}
        className={`composerVoiceButton ${autoSpeak ? "is-active" : ""}`.trim()}
        disabled={disabled}
        onClick={onToggleAutoSpeak}
        title={autoSpeak ? "Disable auto-speak" : "Enable auto-speak"}
        type="button"
      >
        {autoSpeak ? <VolumeX size={16} aria-hidden /> : <Volume2 size={16} aria-hidden />}
      </button>
      <button
        aria-label={realtimeActive ? "Stop realtime voice" : "Start realtime voice"}
        aria-pressed={realtimeActive}
        className={`composerVoiceButton ${realtimeActive ? "is-active" : ""}`.trim()}
        disabled={disabled}
        onClick={onToggleRealtime}
        title={realtimeActive ? "Stop realtime voice" : "Start realtime voice"}
        type="button"
      >
        <Radio size={16} aria-hidden />
      </button>
    </div>
  );
}
