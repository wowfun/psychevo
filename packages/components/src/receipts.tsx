import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ActionButton, IconButton } from "./primitives";
import { RotateCcw, X } from "lucide-react";

export type ActionReceiptInput = {
  message: string;
  tone?: "neutral" | "danger" | undefined;
  undo?: (() => Promise<void> | void) | undefined;
};

type ActionReceipt = ActionReceiptInput & { id: number };
type ActionReceiptApi = {
  available: boolean;
  dismiss(id: number): void;
  push(receipt: ActionReceiptInput): number;
};

const ActionReceiptContext = createContext<ActionReceiptApi | null>(null);

export function useActionReceipts(): ActionReceiptApi {
  const value = useContext(ActionReceiptContext);
  return value ?? { available: false, dismiss: () => undefined, push: () => -1 };
}

export function ActionReceiptProvider({ children, durationMs = 8_000 }: { children: ReactNode; durationMs?: number | undefined }) {
  const nextId = useRef(0);
  const [receipts, setReceipts] = useState<ActionReceipt[]>([]);
  const dismiss = useCallback((id: number) => {
    setReceipts((current) => current.filter((receipt) => receipt.id !== id));
  }, []);
  const push = useCallback((receipt: ActionReceiptInput) => {
    const id = ++nextId.current;
    setReceipts((current) => [...current, { ...receipt, id }].slice(-2));
    return id;
  }, []);
  return (
    <ActionReceiptContext.Provider value={{ available: true, dismiss, push }}>
      {children}
      <aside aria-label="Recent actions" className="pevo-actionReceiptRail">
        {receipts.map((receipt) => (
          <ReceiptRow dismiss={dismiss} durationMs={durationMs} key={receipt.id} receipt={receipt} />
        ))}
      </aside>
    </ActionReceiptContext.Provider>
  );
}

function ReceiptRow({ dismiss, durationMs, receipt }: { dismiss(id: number): void; durationMs: number; receipt: ActionReceipt }) {
  const [pending, setPending] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stopTimer = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
    timeoutRef.current = null;
  }, []);
  const startTimer = useCallback(() => {
    stopTimer();
    timeoutRef.current = setTimeout(() => dismiss(receipt.id), durationMs);
  }, [dismiss, durationMs, receipt.id, stopTimer]);
  useEffect(() => {
    startTimer();
    return stopTimer;
  }, [startTimer, stopTimer]);
  return (
    <div
      className={["pevo-actionReceipt", receipt.tone === "danger" ? "is-danger" : ""].filter(Boolean).join(" ")}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) startTimer();
      }}
      onFocus={stopTimer}
      onMouseEnter={stopTimer}
      onMouseLeave={startTimer}
      role="status"
    >
      <span aria-hidden="true" className="pevo-actionReceiptMarker">•</span>
      <span className="pevo-actionReceiptMessage">{receipt.message}</span>
      {receipt.undo && (
        <ActionButton
          aria-label={`Undo ${receipt.message}`}
          disabled={pending}
          icon={<RotateCcw size={12} />}
          onClick={async () => {
            setPending(true);
            try {
              await receipt.undo?.();
              dismiss(receipt.id);
            } finally {
              setPending(false);
            }
          }}
          pending={pending}
          size="compact"
          variant="ghost"
        >
          Undo
        </ActionButton>
      )}
      <IconButton icon={<X size={12} />} label={`Dismiss ${receipt.message}`} onClick={() => dismiss(receipt.id)} size="compact" />
    </div>
  );
}
