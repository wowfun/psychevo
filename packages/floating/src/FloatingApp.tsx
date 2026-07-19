import {
  GatewayClient,
  emptyThreadSnapshot,
  runThreadInterrupt,
  ThreadController,
  type ThreadTurnControls
} from "@psychevo/client";
import { TranscriptPanel } from "@psychevo/components";
import type { HostCapabilityResult } from "@psychevo/host";
import type { GatewayEvent, GatewayRequestScope, ThreadContextReadResult, ThreadSnapshot } from "@psychevo/protocol";
import { ArrowUp, Camera, FilePlus2, Languages, Maximize2, MessageCircle, Minus, RefreshCcw, Sparkles, Square, TextCursorInput, Wand2, X } from "lucide-react";
import { useEffect, useMemo, useReducer, useRef, useState, type ChangeEvent, type FormEvent, type PointerEvent as ReactPointerEvent } from "react";
import {
  actionPrompt,
  attachmentInputParts,
  capsuleReducer,
  floatingScope,
  initialCapsuleState,
  type CapsuleAction,
  type FloatingAttachment,
  type Rect
} from "./capsule";

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;

const ACTIONS: Array<{ action: CapsuleAction; icon: typeof MessageCircle; label: string }> = [
  { action: "ask", icon: MessageCircle, label: "Ask" },
  { action: "explain", icon: Sparkles, label: "Explain" },
  { action: "translate", icon: Languages, label: "Translate" },
  { action: "rewrite", icon: Wand2, label: "Rewrite" }
];

export interface FloatingActivation {
  activationId: string;
  anchor: Rect | null;
  attachments: FloatingAttachment[];
  cwd: string;
}

export interface FloatingRuntime {
  beginRegionPicker?(): Promise<HostCapabilityResult<Rect | null>>;
  captureRegion?(bounds: Rect): Promise<HostCapabilityResult<{ dataUrl: string; name: string }>>;
  captureSelection(): Promise<FloatingActivation>;
  closeFloatingWindow?(): Promise<void>;
  connectGateway(): Promise<GatewayClient>;
  fitWindowToContent?(size: { width: number; height: number }): Promise<void>;
  initialActivation(): Promise<FloatingActivation>;
  locale?: string;
  openThreadInWorkbench?(threadId: string): Promise<void>;
  startWindowDrag?(): Promise<void>;
  turnControls?(context: FloatingTurnControlsContext): Promise<FloatingTurnPreparation | null>;
}

export interface FloatingTurnControlsContext {
  client: GatewayClient;
  scope: GatewayRequestScope;
  threadId: string | null;
}

export interface FloatingTurnPreparation {
  context: ThreadContextReadResult;
  controls: ThreadTurnControls;
}

export function FloatingApp({ runtime }: { runtime: FloatingRuntime }) {
  const [state, dispatch] = useReducer(capsuleReducer, initialCapsuleState);
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [cwd, setCwd] = useState("");
  const [bridgeReady, setBridgeReady] = useState(false);
  const [transcript, setTranscript] = useState<ThreadSnapshot | null>(null);
  const capsuleRef = useRef<HTMLElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const threadControllerRef = useRef(new ThreadController(null));
  const lastWindowFitRef = useRef<{ width: number; height: number } | null>(null);
  const locale = useMemo(
    () => runtime.locale ?? (typeof navigator === "undefined" ? "system locale" : navigator.language || "system locale"),
    [runtime.locale]
  );

  useEffect(() => {
    const capsule = capsuleRef.current;
    if (!capsule || !runtime.fitWindowToContent || typeof ResizeObserver === "undefined") {
      return;
    }

    let frame: number | null = null;
    const fit = () => {
      frame = null;
      const rect = capsule.getBoundingClientRect();
      const width = Math.max(320, Math.ceil(window.innerWidth || rect.width || 780));
      const height = Math.max(48, Math.ceil(rect.height));
      const last = lastWindowFitRef.current;
      if (last?.width === width && last.height === height) {
        return;
      }
      lastWindowFitRef.current = { height, width };
      void Promise.resolve(runtime.fitWindowToContent?.({ height, width })).catch((error) => {
        dispatch({ type: "error", message: errorMessage(error) });
      });
    };
    const scheduleFit = () => {
      if (frame !== null) {
        return;
      }
      frame = window.requestAnimationFrame(fit);
    };
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(capsule);
    scheduleFit();
    window.addEventListener("resize", scheduleFit);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", scheduleFit);
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
    };
  }, [runtime, state.mode]);

  useEffect(() => {
    let disposed = false;
    let nextClient: GatewayClient | null = null;
    void runtime.connectGateway()
      .then((connectedClient) => {
        if (!disposed) {
          nextClient = connectedClient;
          setClient(nextClient);
          setBridgeReady(true);
        } else {
          connectedClient.close();
        }
      })
      .catch((error) => {
        if (!disposed) {
          setBridgeReady(false);
          dispatch({ type: "error", message: errorMessage(error) });
        }
      });
    return () => {
      disposed = true;
      nextClient?.close();
    };
  }, [runtime]);

  useEffect(() => {
    if (!client) {
      return;
    }
    return client.subscribe((notification) => {
      if (notification.method !== "gateway/event") {
        return;
      }
      const applied = threadControllerRef.current.applyGatewayEvent(notification.params as GatewayEvent);
      if (!applied.applied) {
        return;
      }
      setTranscript(applied.snapshot);
      if (applied.running !== null) {
        dispatch({ running: applied.running, type: "running" });
      }
      if (applied.completed) {
        dispatch({ type: "completed" });
      }
    });
  }, [client]);

  useEffect(() => {
    let disposed = false;
    void runtime.initialActivation()
      .then((activation) => {
        if (disposed) {
          return;
        }
        setCwd(activation.cwd);
        resetTranscript(emptyThreadSnapshot(floatingScope(activation.cwd || "/", activation.activationId), null));
        dispatch({
          activationId: activation.activationId,
          anchor: activation.anchor,
          attachments: activation.attachments,
          type: "freshShow"
        });
      })
      .catch((error) => {
        if (!disposed) {
          dispatch({ type: "error", message: errorMessage(error) });
        }
      });
    return () => {
      disposed = true;
    };
  }, [runtime]);

  async function rescanSelection() {
    try {
      const activation = await runtime.captureSelection();
      setCwd(activation.cwd);
      resetTranscript(emptyThreadSnapshot(floatingScope(activation.cwd || "/", activation.activationId), null));
      dispatch({
        activationId: activation.activationId,
        anchor: activation.anchor,
        attachments: activation.attachments,
        type: "freshShow"
      });
    } catch (error) {
      handleGatewayError(error);
    }
  }

  function resetTranscript(snapshot: ThreadSnapshot | null) {
    threadControllerRef.current.reset(snapshot);
    setTranscript(snapshot);
  }

  async function captureScreenshot() {
    if (!runtime.beginRegionPicker || !runtime.captureRegion) {
      dispatch({ type: "error", message: "Region screenshot capture is unsupported in this host." });
      return;
    }
    try {
      const picker = await runtime.beginRegionPicker();
      if (!picker.ok) {
        dispatch({ type: "error", message: capabilityMessage(picker, "Region screenshot capture is unavailable.") });
        return;
      }
      if (!picker.value) {
        dispatch({ type: "error", message: "Region screenshot capture was canceled." });
        return;
      }
      const capture = await runtime.captureRegion(picker.value);
      if (!capture.ok) {
        dispatch({ type: "error", message: capabilityMessage(capture, "Region screenshot capture failed.") });
        return;
      }
      dispatch({
        attachment: {
          dataUrl: capture.value.dataUrl,
          id: `screenshot:${Date.now()}`,
          kind: "image",
          name: capture.value.name,
          preview: capture.value.name,
          visibleToModel: true
        },
        type: "addAttachment"
      });
    } catch (error) {
      handleGatewayError(error);
    }
  }

  async function submit(action: CapsuleAction) {
    if (!client || !state.activationId) {
      dispatch({ type: "error", message: "Floating is not connected to Gateway." });
      return;
    }
    const prompt = actionPrompt(action, state.draft, locale);
    const input = [
      { type: "text" as const, text: prompt },
      ...attachmentInputParts(state.attachments)
    ];
    const scope = floatingScope(cwd || "/", state.activationId);
    const requestedThreadId = state.threadId;
    try {
      const preparation = await floatingTurnPreparation(
        runtime,
        client,
        scope,
        requestedThreadId ?? null
      );
      threadControllerRef.current.setContext(preparation.context);
      const plan = threadControllerRef.current.beginTurn({
        controls: preparation.controls,
        input,
        optimisticText: prompt,
        scope,
        threadId: requestedThreadId ?? null
      });
      dispatch({ type: "submit", action, prompt });
      setTranscript(plan.snapshot);
      const result = await client.request("turn/start", plan.params).catch((error) => {
        const snapshot = threadControllerRef.current.rejectTurnStart(plan.prepared);
        setTranscript(snapshot);
        throw error;
      });
      const accepted = threadControllerRef.current.acceptTurnStart(result, plan.prepared, "floating turn");
      const threadId = accepted.threadId;
      setTranscript(accepted.snapshot);
      dispatch({ type: "accepted", threadId });
    } catch (error) {
      dispatch({ type: "error", message: errorMessage(error) });
    }
  }

  function startWindowDrag(event: ReactPointerEvent<HTMLElement>) {
    if (event.button !== 0 || !runtime.startWindowDrag) {
      return;
    }
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target?.closest("button, input, textarea, select, a, [data-floating-no-drag='true']")) {
      return;
    }
    event.preventDefault();
    void Promise.resolve(runtime.startWindowDrag()).catch((error) => {
      dispatch({ type: "error", message: errorMessage(error) });
    });
  }

  async function interrupt() {
    if (!client || !state.threadId || !state.activationId) {
      return;
    }
    try {
      await runThreadInterrupt(client, {
        scope: floatingScope(cwd || "/", state.activationId),
        threadId: state.threadId
      });
      dispatch({ running: false, type: "running" });
    } catch (error) {
      dispatch({ type: "error", message: errorMessage(error) });
    }
  }

  async function openInWorkbench() {
    if (!state.threadId || !runtime.openThreadInWorkbench) {
      return;
    }
    try {
      await runtime.openThreadInWorkbench(state.threadId);
    } catch (error) {
      dispatch({ type: "error", message: errorMessage(error) });
    }
  }

  function handleGatewayError(error: unknown) {
    const message = errorMessage(error);
    if (message.includes("Gateway bridge")) {
      setBridgeReady(false);
    }
    dispatch({ type: "error", message });
  }

  async function onFileChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (!file) {
      return;
    }
    dispatch({ attachment: await attachmentFromFile(file), type: "addAttachment" });
  }

  async function closeCapsule() {
    resetTranscript(null);
    dispatch({ type: "close" });
    try {
      await runtime.closeFloatingWindow?.();
    } catch (error) {
      dispatch({ type: "error", message: errorMessage(error) });
    }
  }

  if (state.mode === "hidden") {
    return null;
  }

  const expanded = state.mode === "expanded" || state.mode === "running" || state.mode === "submitting" || Boolean(state.threadId);

  return (
    <main className={expanded ? "pevo-floating pevo-floating-capsule pevo-floating-capsuleExpanded" : "pevo-floating pevo-floating-capsule"} data-mode={state.mode} ref={capsuleRef}>
      <header className="pevo-floating-capsuleToolbar" onPointerDown={startWindowDrag}>
        <span className="pevo-floating-brandMark" aria-label="Psychevo">
          <FloatingLogo />
        </span>
        <div className="pevo-floating-actionRail" aria-label="Floating actions" role="toolbar">
          {ACTIONS.map(({ action, icon: Icon, label }) => (
            <button
              className="pevo-floating-actionButton"
              disabled={!bridgeReady || state.running}
              key={action}
              onClick={() => void submit(action)}
              type="button"
            >
              <Icon aria-hidden="true" size={15} />
              <span>{label}</span>
            </button>
          ))}
        </div>
        <div className="pevo-floating-toolbarDragRegion" aria-hidden="true" />
        <button className="pevo-floating-iconButton" onClick={() => void rescanSelection()} title="Capture selection" type="button">
          <RefreshCcw aria-hidden="true" size={15} />
        </button>
        <button className="pevo-floating-iconButton" onClick={() => void captureScreenshot()} title="Capture region" type="button">
          <Camera aria-hidden="true" size={15} />
        </button>
        {state.threadId && runtime.openThreadInWorkbench && (
          <button aria-label="Open in main window" className="pevo-floating-iconButton" onClick={() => void openInWorkbench()} title="Open in main window" type="button">
            <Maximize2 aria-hidden="true" size={15} />
          </button>
        )}
        <button className="pevo-floating-iconButton" onClick={() => dispatch({ type: "park" })} title="Park" type="button">
          <Minus aria-hidden="true" size={15} />
        </button>
        <button className="pevo-floating-iconButton" onClick={() => void closeCapsule()} title="Close" type="button">
          <X aria-hidden="true" size={15} />
        </button>
      </header>

      {state.mode === "parked" ? (
        <button
          className="pevo-floating-parkedButton"
          aria-label={state.running ? "Floating is running" : state.unseenCompletion ? "Floating answer ready" : "Restore Floating"}
          onClick={() => dispatch({ type: "restore" })}
          title={state.running ? "Running" : state.unseenCompletion ? "Done" : "Restore"}
          type="button"
        >
          <FloatingLogo />
        </button>
      ) : (
        <>
          {state.error && (
            <div className="pevo-floating-errorRow" role="alert">
              <span>{state.error}</span>
              <button onClick={() => dispatch({ type: "clearError" })} type="button">Dismiss</button>
            </div>
          )}

          <AttachmentStrip
            attachments={state.attachments}
            onAttach={() => fileInputRef.current?.click()}
            onRemove={(id) => dispatch({ id, type: "removeAttachment" })}
          />

          {expanded && (
            <section className="pevo-floating-answerPanel" aria-live="polite">
              <TranscriptPanel
                {...(transcript ? { activity: transcript.activity } : {})}
                entries={transcript?.entries ?? []}
                onCopyText={(text) => navigator.clipboard?.writeText(text)}
                threadId={state.threadId}
              />
            </section>
          )}

          <form className="pevo-floating-promptRow" onSubmit={(event: FormEvent) => {
            event.preventDefault();
            void submit("ask");
          }}>
            <TextCursorInput aria-hidden="true" size={16} />
            <input
              aria-label="Ask Psychevo"
              onChange={(event) => dispatch({ draft: event.target.value, type: "setDraft" })}
              placeholder={expanded ? "Follow up..." : "Ask on selection"}
              value={state.draft}
            />
            {state.running ? (
              <button className="pevo-floating-sendButton" onClick={() => void interrupt()} title="Interrupt" type="button">
                <Square aria-hidden="true" size={13} />
              </button>
            ) : (
              <button className="pevo-floating-sendButton" disabled={!bridgeReady} title="Ask" type="submit">
                <ArrowUp aria-hidden="true" size={15} />
              </button>
            )}
          </form>

          <input className="pevo-floating-hiddenFileInput" onChange={(event) => void onFileChange(event)} ref={fileInputRef} type="file" />
        </>
      )}
    </main>
  );
}

function FloatingLogo() {
  return <img className="pevo-floating-logo" src={logoUrl} alt="" aria-hidden="true" />;
}

async function floatingTurnPreparation(
  runtime: FloatingRuntime,
  client: GatewayClient,
  scope: GatewayRequestScope,
  threadId: string | null
): Promise<FloatingTurnPreparation> {
  if (runtime.turnControls) {
    const prepared = await runtime.turnControls({ client, scope, threadId });
    if (prepared) return prepared;
  }
  const discovery = await client.request("thread/context/read", {
    threadId,
    target: null,
    scope
  });
  let context = discovery;
  if (!threadId) {
    const discoveryController = new ThreadController();
    discoveryController.setContext(discovery);
    const discoveryTargetId = discovery.selectedTargetId ?? discovery.suggestedTargetId;
    const target = discoveryTargetId
      ? discoveryController.contextReadTarget(discoveryTargetId)
      : null;
    if (!target) {
      throw new Error("Gateway did not provide a canonical Floating Agent target.");
    }
    context = await client.request("thread/context/read", {
      threadId: null,
      target,
      scope
    });
  }
  const controller = new ThreadController();
  controller.setContext(context);
  return {
    context,
    controls: controller.turnControls(context.selectedTargetId ?? "", {})
  };
}

function AttachmentStrip({
  attachments,
  onAttach,
  onRemove
}: {
  attachments: FloatingAttachment[];
  onAttach(): void;
  onRemove(id: string): void;
}) {
  return (
    <div className="pevo-floating-attachmentStrip">
      {attachments.map((attachment) => (
        <span className={`pevo-floating-attachmentChip is-${attachment.kind}`} key={attachment.id} title={attachment.preview}>
          {attachment.kind === "image" && (
            <img className="pevo-floating-attachmentThumb" src={attachment.dataUrl} alt="" />
          )}
          <span>{attachment.name}</span>
          <button aria-label={`Remove ${attachment.name}`} onClick={() => onRemove(attachment.id)} type="button">
            <X aria-hidden="true" size={12} />
          </button>
        </span>
      ))}
      <button className="pevo-floating-attachButton" onClick={onAttach} type="button">
        <FilePlus2 aria-hidden="true" size={14} />
        Add file
      </button>
    </div>
  );
}

async function attachmentFromFile(file: File): Promise<FloatingAttachment> {
  const id = `${Date.now()}:${file.name}:${file.size}`;
  const sizeLabel = formatBytes(file.size);
  if (file.type.startsWith("image/")) {
    return {
      dataUrl: await fileToDataUrl(file),
      id,
      kind: "image",
      name: file.name || "image",
      preview: `${file.name || "image"} ${sizeLabel}`,
      sizeLabel,
      visibleToModel: true
    };
  }
  const text = file.type.startsWith("text/") || /\.(md|txt|json|toml|yaml|yml|rs|ts|tsx|js|jsx|py)$/i.test(file.name)
    ? await file.slice(0, 256 * 1024).text()
    : null;
  return {
    id,
    kind: "file",
    mime: file.type || null,
    name: file.name || "file",
    preview: text?.slice(0, 160) || `${file.name || "file"} ${sizeLabel}`,
    sizeLabel,
    text,
    visibleToModel: true
  };
}

function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result ?? "")), { once: true });
    reader.addEventListener("error", () => reject(reader.error ?? new Error("failed to read file")), { once: true });
    reader.readAsDataURL(file);
  });
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const kib = bytes / 1024;
  if (kib < 1024) {
    return `${Math.round(kib * 10) / 10} KiB`;
  }
  const mib = kib / 1024;
  return `${Math.round(mib * 10) / 10} MiB`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function capabilityMessage(result: { message?: string; reason: string }, fallback: string): string {
  if (result.message?.trim()) {
    return result.message;
  }
  switch (result.reason) {
    case "permissionDenied":
      return "Region screenshot permission was denied.";
    case "canceled":
      return "Region screenshot capture was canceled.";
    case "unsupported":
      return "Region screenshot capture is unsupported in this host.";
    case "unavailable":
      return "Region screenshot capture is unavailable.";
    default:
      return fallback;
  }
}
