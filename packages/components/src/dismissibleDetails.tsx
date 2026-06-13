import {
  useEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type DetailsHTMLAttributes,
  type ReactNode
} from "react";

export type DismissibleDetailsControls = {
  close(): void;
  open: boolean;
};

export interface DismissibleDetailsProps
  extends Omit<DetailsHTMLAttributes<HTMLDetailsElement>, "children" | "onToggle" | "open"> {
  children: (controls: DismissibleDetailsControls) => ReactNode;
  summary: ReactNode;
  summaryProps?: ComponentPropsWithoutRef<"summary">;
}

export function DismissibleDetails({
  children,
  summary,
  summaryProps,
  ...props
}: DismissibleDetailsProps) {
  const [open, setOpen] = useState(false);
  const detailsRef = useRef<HTMLDetailsElement | null>(null);
  const summaryRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    function onPointerDown(event: MouseEvent) {
      if (detailsRef.current?.contains(event.target as Node)) {
        return;
      }
      setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      setOpen(false);
      summaryRef.current?.focus();
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <details
      {...props}
      ref={detailsRef}
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary {...summaryProps} ref={summaryRef}>
        {summary}
      </summary>
      {children({ close: () => setOpen(false), open })}
    </details>
  );
}
