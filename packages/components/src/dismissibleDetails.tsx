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
    function onPointerDown(event: MouseEvent | PointerEvent) {
      const details = detailsRef.current;
      if (!details?.open || details.contains(event.target as Node)) {
        return;
      }
      details.open = false;
      setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      const details = detailsRef.current;
      if (event.key !== "Escape" || !details?.open) {
        return;
      }
      event.preventDefault();
      details.open = false;
      setOpen(false);
      summaryRef.current?.focus();
    }
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, []);

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
