import type { ButtonHTMLAttributes } from "react";

export function IconButton({
  children,
  danger,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { danger?: boolean }) {
  const label = props["aria-label"] ?? (typeof props.title === "string" ? props.title : undefined);
  return (
    <button
      {...props}
      aria-label={label}
      className={`pevo-iconButton ${danger ? "is-danger" : ""} ${props.className ?? ""}`.trim()}
      type={props.type ?? "button"}
    >
      {children}
    </button>
  );
}
