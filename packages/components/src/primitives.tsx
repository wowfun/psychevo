import { cloneElement, isValidElement, useId } from "react";
import type { ButtonHTMLAttributes, HTMLAttributes, ReactElement, ReactNode } from "react";

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

export type ActionButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label"> & {
  active?: boolean | undefined;
  ariaLabel?: string | undefined;
  busy?: boolean | undefined;
  icon?: ReactNode;
  iconOnly?: boolean | undefined;
  size?: "default" | "compact" | undefined;
  tooltip?: string | undefined;
  variant?: "neutral" | "primary" | "ghost" | "danger" | undefined;
  "aria-label"?: string | undefined;
};

export function ActionButton({
  active = false,
  ariaLabel,
  busy = false,
  children,
  className,
  disabled,
  icon,
  iconOnly = false,
  size = "default",
  title,
  tooltip,
  type = "button",
  variant = "neutral",
  ...props
}: ActionButtonProps) {
  const explicitLabel = ariaLabel ?? props["aria-label"];
  const iconOnlyLabel = explicitLabel ?? (typeof title === "string" ? title : undefined) ?? tooltip;
  const unavailable = disabled || busy;
  return (
    <button
      {...props}
      aria-busy={busy || undefined}
      aria-label={iconOnly ? iconOnlyLabel : explicitLabel ?? undefined}
      aria-pressed={active || undefined}
      className={[
        "pevo-actionButton",
        `pevo-actionButton--${variant}`,
        `pevo-actionButton--${size}`,
        iconOnly ? "pevo-actionButton--iconOnly" : "",
        active ? "is-active" : "",
        busy ? "is-busy" : "",
        className ?? ""
      ].filter(Boolean).join(" ")}
      data-active={active || undefined}
      data-variant={variant}
      disabled={unavailable}
      title={typeof title === "string" ? title : tooltip}
      type={type}
    >
      {icon && <span aria-hidden="true" className="pevo-actionButtonIcon">{icon}</span>}
      {iconOnly ? children && <span className="pevo-srOnly">{children}</span> : children && <span className="pevo-actionButtonLabel">{children}</span>}
    </button>
  );
}

export type FormFieldProps = {
  children: ReactElement | ReactNode;
  className?: string | undefined;
  controlId?: string | undefined;
  error?: ReactNode;
  hint?: ReactNode;
  label: ReactNode;
};

export function FormField({
  children,
  className,
  controlId,
  error,
  hint,
  label
}: FormFieldProps) {
  const generatedId = useId();
  const id = controlId ?? `pevo-field-${generatedId}`;
  const labelId = `${id}-label`;
  const hintId = hint ? `${id}-hint` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const child = isValidElement<Record<string, unknown>>(children)
    ? cloneElement(children, {
        id: typeof children.props.id === "string" ? children.props.id : id,
        "aria-labelledby": mergeIds(children.props["aria-labelledby"], labelId),
        "aria-describedby": mergeIds(children.props["aria-describedby"], hintId, errorId),
        "aria-invalid": error ? true : children.props["aria-invalid"]
      })
    : children;
  return (
    <label className={`pevo-formField${error ? " is-invalid" : ""}${className ? ` ${className}` : ""}`} htmlFor={id}>
      <span className="pevo-formFieldCopy">
        <span className="pevo-formFieldLabel" id={labelId}>{label}</span>
        {hint && <span className="pevo-formFieldHint" id={hintId}>{hint}</span>}
      </span>
      <span className="pevo-formFieldControl">{child}</span>
      {error && <span className="pevo-formFieldError" id={errorId} role="alert">{error}</span>}
    </label>
  );
}

function mergeIds(...ids: Array<unknown>): string | undefined {
  const parts = ids
    .flatMap((id) => (typeof id === "string" ? id.split(/\s+/) : []))
    .filter(Boolean);
  return parts.length > 0 ? [...new Set(parts)].join(" ") : undefined;
}

export type CreatePanelProps = HTMLAttributes<HTMLElement> & {
  description?: ReactNode;
  footer?: ReactNode;
  icon?: ReactNode;
  layout?: "inline" | "side" | "dialog" | undefined;
  onClose?: (() => void) | undefined;
  title: ReactNode;
};

export function CreatePanel({
  children,
  className,
  description,
  footer,
  icon,
  layout = "inline",
  onClose,
  title,
  ...props
}: CreatePanelProps) {
  const headingId = useId();
  return (
    <section
      {...props}
      aria-labelledby={headingId}
      className={[
        "pevo-createPanel",
        `pevo-createPanel--${layout}`,
        className ?? ""
      ].filter(Boolean).join(" ")}
      role={layout === "dialog" ? "dialog" : props.role ?? "group"}
    >
      <header className="pevo-createPanelHeader">
        <div className="pevo-createPanelTitleGroup">
          {icon && <span aria-hidden="true" className="pevo-createPanelIcon">{icon}</span>}
          <div>
            <h3 className="pevo-createPanelTitle" id={headingId}>{title}</h3>
            {description && <p className="pevo-createPanelDescription">{description}</p>}
          </div>
        </div>
        {onClose && (
          <ActionButton ariaLabel="Close" iconOnly onClick={onClose} size="compact" tooltip="Close" variant="ghost">
            Close
          </ActionButton>
        )}
      </header>
      <div className="pevo-createPanelBody">{children}</div>
      {footer && <footer className="pevo-createPanelFooter">{footer}</footer>}
    </section>
  );
}

export type SwitchProps = {
  checked: boolean;
  label: string;
  ariaLabel?: string | undefined;
  className?: string | undefined;
  disabled?: boolean | undefined;
  icon?: ReactNode;
  pending?: boolean | undefined;
  showLabel?: boolean | undefined;
  size?: "default" | "compact" | undefined;
  onCheckedChange?(checked: boolean): void;
};

export function Switch({
  ariaLabel,
  checked,
  className,
  disabled = false,
  icon,
  label,
  onCheckedChange,
  pending = false,
  showLabel = true,
  size = "default"
}: SwitchProps) {
  const unavailable = disabled || pending;
  return (
    <button
      aria-busy={pending || undefined}
      aria-checked={checked}
      aria-label={ariaLabel ?? (showLabel ? undefined : label)}
      className={[
        "pevo-switch",
        `pevo-switch--${size}`,
        checked ? "is-on" : "",
        pending ? "is-pending" : "",
        className ?? ""
      ].filter(Boolean).join(" ")}
      disabled={unavailable}
      onClick={() => {
        if (!unavailable) onCheckedChange?.(!checked);
      }}
      role="switch"
      type="button"
    >
      <span aria-hidden="true" className="pevo-switchTrack">
        <span className="pevo-switchThumb" />
      </span>
      {showLabel && (
        <span className="pevo-switchLabel">
          {icon && <span aria-hidden="true" className="pevo-switchLabelIcon">{icon}</span>}
          <span>{label}</span>
        </span>
      )}
    </button>
  );
}
