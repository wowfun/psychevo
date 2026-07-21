import { X } from "lucide-react";
import { cloneElement, isValidElement, useId } from "react";
import type { AnchorHTMLAttributes, ButtonHTMLAttributes, HTMLAttributes, ReactElement, ReactNode, Ref } from "react";

export type ControlSize = "compact" | "default";
export type ButtonVariant = "primary" | "secondary" | "ghost" | "caution" | "danger" | "interrupt";

export type ActionButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  block?: boolean | undefined;
  icon?: ReactNode;
  pending?: boolean | undefined;
  ref?: Ref<HTMLButtonElement> | undefined;
  size?: ControlSize | undefined;
  variant?: ButtonVariant | undefined;
};

export function ActionButton({
  block = false,
  children,
  className,
  disabled,
  icon,
  pending = false,
  ref,
  size = "default",
  type = "button",
  variant = "secondary",
  ...props
}: ActionButtonProps) {
  const unavailable = disabled || pending;
  return (
    <button
      {...props}
      aria-busy={pending || undefined}
      className={[
        "pevo-actionButton",
        `pevo-actionButton--${variant}`,
        `pevo-actionButton--${size}`,
        block ? "pevo-actionButton--block" : "",
        pending ? "is-pending" : "",
        className ?? ""
      ].filter(Boolean).join(" ")}
      data-pending={pending || undefined}
      data-variant={variant}
      disabled={unavailable}
      ref={ref}
      type={type}
    >
      {icon && <span aria-hidden="true" className="pevo-actionButtonIcon">{icon}</span>}
      {children && <span className="pevo-actionButtonLabel">{children}</span>}
      {pending && <span aria-hidden="true" className="pevo-controlSpinner" />}
    </button>
  );
}

export type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label" | "children" | "title"> & {
  icon: ReactNode;
  label: string;
  pending?: boolean | undefined;
  shape?: "rounded" | "circle" | undefined;
  size?: ControlSize | undefined;
  tooltip?: string | undefined;
  variant?: ButtonVariant | undefined;
};

export function IconButton({
  className,
  disabled,
  icon,
  label,
  pending = false,
  shape = "rounded",
  size = "default",
  type = "button",
  tooltip,
  variant = "ghost",
  ...props
}: IconButtonProps) {
  return (
    <button
      {...props}
      aria-busy={pending || undefined}
      aria-label={label}
      className={[
        "pevo-iconButton",
        `pevo-iconButton--${variant}`,
        `pevo-iconButton--${size}`,
        `pevo-iconButton--${shape}`,
        pending ? "is-pending" : "",
        className ?? ""
      ].filter(Boolean).join(" ")}
      data-pending={pending || undefined}
      data-variant={variant}
      disabled={disabled || pending}
      title={tooltip ?? label}
      type={type}
    >
      <span aria-hidden="true" className="pevo-iconButtonIcon">{icon}</span>
      {pending && <span aria-hidden="true" className="pevo-controlSpinner" />}
    </button>
  );
}

export type ToggleButtonProps = Omit<IconButtonProps, "onClick" | "variant"> & {
  children?: ReactNode;
  onPressedChange(pressed: boolean): void;
  pressed: boolean;
};

export function ToggleButton({
  children,
  className,
  onPressedChange,
  pressed,
  ...props
}: ToggleButtonProps) {
  const { icon, label, pending, shape, size, tooltip, ...buttonProps } = props;
  if (children) {
    return (
      <ActionButton
        {...buttonProps}
        aria-label={label}
        aria-pressed={pressed}
        className={["pevo-toggleButton", pressed ? "is-selected" : "", className ?? ""].filter(Boolean).join(" ")}
        icon={icon}
        onClick={() => onPressedChange(!pressed)}
        pending={pending}
        size={size}
        title={tooltip ?? label}
        variant="ghost"
      >
        {children}
      </ActionButton>
    );
  }
  return (
    <IconButton
      {...buttonProps}
      aria-pressed={pressed}
      className={["pevo-toggleButton", pressed ? "is-selected" : "", className ?? ""].filter(Boolean).join(" ")}
      icon={icon}
      label={label}
      onClick={() => onPressedChange(!pressed)}
      pending={pending}
      shape={shape}
      size={size}
      tooltip={tooltip}
    />
  );
}

export type DisclosureButtonProps = Omit<ActionButtonProps, "aria-expanded" | "aria-controls" | "onClick"> & {
  controls: string;
  expanded: boolean;
  label: string;
  onExpandedChange(expanded: boolean): void;
};

export function DisclosureButton({
  children,
  className,
  controls,
  expanded,
  label,
  onExpandedChange,
  variant = "ghost",
  ...props
}: DisclosureButtonProps) {
  return (
    <ActionButton
      {...props}
      aria-controls={controls}
      aria-expanded={expanded}
      aria-label={label}
      className={["pevo-disclosureButton", expanded ? "is-expanded" : "", className ?? ""].filter(Boolean).join(" ")}
      onClick={() => onExpandedChange(!expanded)}
      variant={variant}
    >
      {children ?? label}
    </ActionButton>
  );
}

export type NavItemProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "onClick"> & {
  current: boolean;
  icon?: ReactNode;
  label: ReactNode;
  meta?: ReactNode;
  onSelect(): void;
};

export function NavItem({ className, current, icon, label, meta, onSelect, type = "button", ...props }: NavItemProps) {
  return (
    <button
      {...props}
      aria-current={current ? "page" : undefined}
      className={["pevo-navItem", current ? "is-current" : "", className ?? ""].filter(Boolean).join(" ")}
      onClick={onSelect}
      type={type}
    >
      {icon && <span aria-hidden="true" className="pevo-navItemIcon">{icon}</span>}
      <span className="pevo-navItemLabel">{label}</span>
      {meta && <span className="pevo-navItemMeta">{meta}</span>}
    </button>
  );
}

export type ActionLinkProps = AnchorHTMLAttributes<HTMLAnchorElement> & {
  external?: boolean | undefined;
  icon?: ReactNode;
  tone?: "neutral" | "danger" | undefined;
};

export function ActionLink({ children, className, external = false, icon, rel, target, tone = "neutral", ...props }: ActionLinkProps) {
  return (
    <a
      {...props}
      className={["pevo-actionLink", `pevo-actionLink--${tone}`, className ?? ""].filter(Boolean).join(" ")}
      rel={external ? "noopener noreferrer" : rel}
      target={external ? "_blank" : target}
    >
      {icon && <span aria-hidden="true" className="pevo-actionLinkIcon">{icon}</span>}
      <span>{children}</span>
    </a>
  );
}

export type SegmentOption<Value extends string> = {
  disabled?: boolean | undefined;
  icon?: ReactNode;
  label: string;
  value: Value;
};

export type SegmentedControlProps<Value extends string> = {
  className?: string | undefined;
  disabled?: boolean | undefined;
  label: string;
  onValueChange(value: Value): void;
  options: readonly SegmentOption<Value>[];
  value: Value;
};

export function SegmentedControl<Value extends string>({
  className,
  disabled = false,
  label,
  onValueChange,
  options,
  value
}: SegmentedControlProps<Value>) {
  function move(currentValue: Value, key: string, group: HTMLElement) {
    const enabled = options.filter((option) => !option.disabled);
    const currentIndex = enabled.findIndex((option) => option.value === currentValue);
    if (enabled.length === 0 || currentIndex < 0) return;
    const nextIndex = key === "Home"
      ? 0
      : key === "End"
        ? enabled.length - 1
        : key === "ArrowRight" || key === "ArrowDown"
          ? (currentIndex + 1) % enabled.length
          : (currentIndex - 1 + enabled.length) % enabled.length;
    const next = enabled[nextIndex];
    if (!next) return;
    onValueChange(next.value);
    [...group.querySelectorAll<HTMLElement>("[data-segment-value]")]
      .find((element) => element.dataset.segmentValue === next.value)
      ?.focus();
  }

  return (
    <div aria-label={label} className={["pevo-segmentedControl", className ?? ""].filter(Boolean).join(" ")} role="radiogroup">
      {options.map((option) => {
        const checked = option.value === value;
        return (
          <button
            aria-checked={checked}
            className={checked ? "is-selected" : undefined}
            data-segment-value={option.value}
            disabled={disabled || option.disabled}
            key={option.value}
            onClick={() => onValueChange(option.value)}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              move(option.value, event.key, event.currentTarget.parentElement!);
            }}
            role="radio"
            tabIndex={checked ? 0 : -1}
            type="button"
          >
            {option.icon && <span aria-hidden="true" className="pevo-segmentedControlIcon">{option.icon}</span>}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}

export type TabOption<Value extends string> = SegmentOption<Value> & {
  panelId?: string | undefined;
};

export type TabsProps<Value extends string> = Omit<SegmentedControlProps<Value>, "options"> & {
  options: readonly TabOption<Value>[];
};

export function Tabs<Value extends string>({ className, disabled = false, label, onValueChange, options, value }: TabsProps<Value>) {
  function move(currentValue: Value, key: string, list: HTMLElement) {
    const enabled = options.filter((option) => !option.disabled);
    const currentIndex = enabled.findIndex((option) => option.value === currentValue);
    if (enabled.length === 0 || currentIndex < 0) return;
    const nextIndex = key === "Home" ? 0
      : key === "End" ? enabled.length - 1
        : key === "ArrowRight" || key === "ArrowDown" ? (currentIndex + 1) % enabled.length
          : (currentIndex - 1 + enabled.length) % enabled.length;
    const next = enabled[nextIndex];
    if (!next) return;
    onValueChange(next.value);
    [...list.querySelectorAll<HTMLElement>("[data-tab-value]")]
      .find((element) => element.dataset.tabValue === next.value)
      ?.focus();
  }
  return (
    <div aria-label={label} className={["pevo-tabs", className ?? ""].filter(Boolean).join(" ")} role="tablist">
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            aria-controls={option.panelId}
            aria-selected={selected}
            className={selected ? "is-selected" : undefined}
            data-tab-value={option.value}
            disabled={disabled || option.disabled}
            key={option.value}
            onClick={() => onValueChange(option.value)}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              move(option.value, event.key, event.currentTarget.parentElement!);
            }}
            role="tab"
            tabIndex={selected ? 0 : -1}
            type="button"
          >
            {option.icon && <span aria-hidden="true" className="pevo-tabsIcon">{option.icon}</span>}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
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
          <IconButton icon={<X size={14} />} label="Close" onClick={onClose} size="compact" />
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
  className?: string | undefined;
  disabled?: boolean | undefined;
  icon?: ReactNode;
  pending?: boolean | undefined;
  showLabel?: boolean | undefined;
  size?: "default" | "compact" | undefined;
  onCheckedChange?(checked: boolean): void;
};

export function Switch({
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
      aria-label={showLabel ? undefined : label}
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
