import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode
} from "react";
import { createPortal } from "react-dom";

export type WorkspaceFileMenuItem<Id extends string = string> = {
  disabled?: boolean;
  id: Id;
  label: string;
  separatorBefore?: boolean;
};

export function WorkspaceFileContextMenu<Id extends string>({
  anchor,
  ariaLabel,
  error,
  items,
  loading,
  onClose,
  onSelect
}: {
  anchor: { element: HTMLButtonElement; x: number; y: number };
  ariaLabel: string;
  error?: string | null;
  items: WorkspaceFileMenuItem<Id>[];
  loading: boolean;
  onClose(): void;
  onSelect(id: Id): void;
}) {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState(() => ({ left: anchor.x, top: anchor.y, ready: false }));
  const itemSignature = items
    .map((item) => `${item.id}:${item.disabled === true}:${item.separatorBefore === true}:${item.label}`)
    .join("\n");

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) {
      return;
    }
    const bounds = menu.getBoundingClientRect();
    const margin = 8;
    const nextLeft = Math.max(margin, Math.min(anchor.x, window.innerWidth - bounds.width - margin));
    const nextTop = Math.max(margin, Math.min(anchor.y, window.innerHeight - bounds.height - margin));
    setPosition((current) => (
      current.left === nextLeft && current.top === nextTop && current.ready
        ? current
        : { left: nextLeft, top: nextTop, ready: true }
    ));
  }, [anchor.x, anchor.y, error, itemSignature, loading]);

  useEffect(() => {
    return () => anchor.element.focus();
  }, [anchor.element]);

  useEffect(() => {
    const firstItem = menuRef.current?.querySelector<HTMLButtonElement>("button[role='menuitem']:not(:disabled)");
    (firstItem ?? menuRef.current)?.focus();
  }, [itemSignature, loading]);

  useEffect(() => {
    function handlePointerDown(event: PointerEvent) {
      if (!menuRef.current?.contains(event.target as Node)) {
        onClose();
      }
    }
    function close() {
      onClose();
    }
    document.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [onClose]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (!(["ArrowDown", "ArrowUp", "Home", "End"] as string[]).includes(event.key)) {
      return;
    }
    const enabledItems = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("button[role='menuitem']:not(:disabled)") ?? [])];
    if (enabledItems.length === 0) {
      return;
    }
    event.preventDefault();
    const currentIndex = enabledItems.indexOf(document.activeElement as HTMLButtonElement);
    let nextIndex = 0;
    if (event.key === "End") {
      nextIndex = enabledItems.length - 1;
    } else if (event.key === "ArrowUp") {
      nextIndex = currentIndex <= 0 ? enabledItems.length - 1 : currentIndex - 1;
    } else if (event.key === "ArrowDown") {
      nextIndex = currentIndex < 0 || currentIndex === enabledItems.length - 1 ? 0 : currentIndex + 1;
    }
    enabledItems[nextIndex]?.focus();
  }

  let content: ReactNode;
  if (loading) {
    content = <div aria-disabled="true" className="workspaceFileContextMenuStatus" role="menuitem">Detecting applications…</div>;
  } else if (items.length === 0) {
    content = <div aria-disabled="true" className="workspaceFileContextMenuStatus" role="menuitem">No external actions available.</div>;
  } else {
    content = items.map((item) => (
      <div className={item.separatorBefore ? "has-separator" : undefined} key={item.id}>
        <button
          disabled={item.disabled}
          onClick={() => onSelect(item.id)}
          role="menuitem"
          type="button"
        >
          {item.label}
        </button>
      </div>
    ));
  }

  return createPortal(
    <div
      aria-label={ariaLabel}
      className="workspaceFileContextMenu"
      onKeyDown={handleKeyDown}
      ref={menuRef}
      role="menu"
      tabIndex={-1}
      style={{
        left: position.left,
        top: position.top,
        visibility: position.ready ? "visible" : "hidden"
      }}
    >
      {content}
      {error && <p className="workspaceFileContextMenuError" role="alert">{error}</p>}
    </div>,
    document.body
  );
}
