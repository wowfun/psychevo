import { createContext, useCallback, useContext, useEffect, useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ActionButton, IconButton } from "./primitives";
import { X } from "lucide-react";

export type ConfirmDialogProps = {
  cancelLabel?: string | undefined;
  confirmLabel: string;
  description: ReactNode;
  disabled?: boolean | undefined;
  onCancel(): void;
  onConfirm(): void;
  open: boolean;
  pending?: boolean | undefined;
  title: string;
  tone?: "caution" | "danger" | undefined;
};

export type ConfirmActionRequest = Pick<ConfirmDialogProps, "confirmLabel" | "description" | "title" | "tone"> & {
  action?: (() => Promise<unknown> | unknown) | undefined;
};
type PendingConfirmation = ConfirmActionRequest & {
  reject(reason?: unknown): void;
  resolve(value: boolean): void;
};
export type ConfirmAction = (request: ConfirmActionRequest) => Promise<boolean>;
const ConfirmActionContext = createContext<ConfirmAction | null>(null);

export function useConfirmAction(): ConfirmAction {
  const confirm = useContext(ConfirmActionContext);
  return confirm ?? (() => Promise.resolve(false));
}

export function ConfirmActionProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<PendingConfirmation | null>(null);
  const [actionPending, setActionPending] = useState(false);
  const actionPendingRef = useRef(false);
  const confirm = useCallback<ConfirmAction>((request) => new Promise((resolve, reject) => {
    if (actionPendingRef.current) {
      resolve(false);
      return;
    }
    setPending((current) => {
      current?.resolve(false);
      return { ...request, reject, resolve };
    });
  }), []);
  const settle = useCallback((value: boolean) => {
    if (actionPendingRef.current) return;
    setPending((current) => {
      current?.resolve(value);
      return null;
    });
  }, []);
  const runConfirmedAction = useCallback(async () => {
    const current = pending;
    if (!current || actionPendingRef.current) return;
    if (!current.action) {
      settle(true);
      return;
    }
    actionPendingRef.current = true;
    setActionPending(true);
    try {
      await current.action();
      current.resolve(true);
    } catch (error) {
      current.reject(error);
    } finally {
      setPending((active) => active === current ? null : active);
      actionPendingRef.current = false;
      setActionPending(false);
    }
  }, [pending, settle]);
  return (
    <ConfirmActionContext.Provider value={confirm}>
      {children}
      <ConfirmDialog
        confirmLabel={pending?.confirmLabel ?? "Confirm"}
        description={pending?.description ?? null}
        onCancel={() => settle(false)}
        onConfirm={() => void runConfirmedAction()}
        open={Boolean(pending)}
        pending={actionPending}
        title={pending?.title ?? "Confirm action"}
        tone={pending?.tone}
      />
    </ConfirmActionContext.Provider>
  );
}

export function ConfirmDialog({
  cancelLabel = "Cancel",
  confirmLabel,
  description,
  disabled = false,
  onCancel,
  onConfirm,
  open,
  pending = false,
  title,
  tone = "caution"
}: ConfirmDialogProps) {
  const titleId = useId();
  const descriptionId = useId();
  const dialogRef = useRef<HTMLElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const interactionDisabled = disabled || pending;
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cancelRef.current?.focus();
    return () => previous?.focus();
  }, [open]);
  if (!open) return null;
  return (
    <div className="pevo-modalBackdrop" role="presentation">
      <section
        aria-busy={pending || undefined}
        aria-describedby={descriptionId}
        aria-labelledby={titleId}
        aria-modal="true"
        className="pevo-confirmDialog"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !interactionDisabled) {
            event.preventDefault();
            onCancel();
          }
          if (event.key !== "Tab") return;
          const focusable = [...event.currentTarget.querySelectorAll<HTMLElement>("button:not(:disabled)")];
          if (focusable.length === 0) return;
          const first = focusable[0]!;
          const last = focusable[focusable.length - 1]!;
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }}
        ref={dialogRef}
        role="dialog"
      >
        <header className="pevo-confirmDialogHeader">
          <div>
            <p className="pevo-confirmDialogKicker">Confirm mutation</p>
            <h2 id={titleId}>{title}</h2>
          </div>
          <IconButton disabled={interactionDisabled} icon={<X size={15} />} label="Close" onClick={onCancel} size="compact" />
        </header>
        <div className="pevo-confirmDialogBody" id={descriptionId}>{description}</div>
        <footer className="pevo-confirmDialogFooter">
          <ActionButton disabled={interactionDisabled} onClick={onCancel} ref={cancelRef} variant="ghost">{cancelLabel}</ActionButton>
          <ActionButton disabled={disabled} onClick={onConfirm} pending={pending} variant={tone}>{confirmLabel}</ActionButton>
        </footer>
      </section>
    </div>
  );
}

export type MenuItem = {
  disabled?: boolean | undefined;
  id: string;
  label: string;
  onSelect(): void;
  tone?: "neutral" | "danger" | undefined;
};

export type MenuProps = {
  className?: string | undefined;
  items: readonly MenuItem[];
  label: string;
  onOpenChange(open: boolean): void;
  open: boolean;
};

export function Menu({ className, items, label, onOpenChange, open }: MenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')?.focus();
  }, [open]);
  if (!open) return null;
  return (
    <div
      aria-label={label}
      className={["pevo-menu", className ?? ""].filter(Boolean).join(" ")}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onOpenChange(false);
          return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const enabled = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)')];
        if (enabled.length === 0) return;
        const current = enabled.indexOf(document.activeElement as HTMLElement);
        const next = event.key === "Home" ? 0
          : event.key === "End" ? enabled.length - 1
            : event.key === "ArrowDown" ? (current + 1 + enabled.length) % enabled.length
              : (current - 1 + enabled.length) % enabled.length;
        enabled[next]?.focus();
      }}
      ref={menuRef}
      role="menu"
    >
      {items.map((item) => (
        <button
          className={item.tone === "danger" ? "is-danger" : undefined}
          disabled={item.disabled}
          key={item.id}
          onClick={() => {
            item.onSelect();
            onOpenChange(false);
          }}
          role="menuitem"
          tabIndex={-1}
          type="button"
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
