// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ActionButton,
  ActionLink,
  CreatePanel,
  DisclosureButton,
  FormField,
  IconButton,
  NavItem,
  SegmentedControl,
  Switch,
  Tabs,
  ToggleButton
} from "./primitives";

afterEach(() => {
  cleanup();
});

describe("ActionButton", () => {
  it("renders a visible command without toggle semantics and keeps its label while pending", () => {
    render(
      <ActionButton block icon={<span data-testid="icon" />} pending variant="primary">
        Save changes
      </ActionButton>
    );

    const button = screen.getByRole("button", { name: "Save changes" }) as HTMLButtonElement;
    expect(button.type).toBe("button");
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("aria-busy")).toBe("true");
    expect(button.hasAttribute("aria-pressed")).toBe(false);
    expect(button.className).toContain("pevo-actionButton--primary");
    expect(button.className).toContain("pevo-actionButton--block");
    expect(screen.getByTestId("icon")).toBeTruthy();
  });

  it("renders icon commands with one stable label and tooltip", () => {
    render(
      <IconButton icon={<span data-testid="icon" />} label="Add backend" />
    );

    const button = screen.getByRole("button", { name: "Add backend" });
    expect(button.getAttribute("title")).toBe("Add backend");
    expect(screen.getByTestId("icon")).toBeTruthy();
  });
});

describe("semantic controls", () => {
  it("keeps pressed, expanded, and current state on separate interfaces", () => {
    const onPressedChange = vi.fn();
    const onExpandedChange = vi.fn();
    const onSelect = vi.fn();
    render(
      <>
        <ToggleButton icon={<span />} label="Preview" onPressedChange={onPressedChange} pressed={false} />
        <DisclosureButton controls="details" expanded={false} label="Details" onExpandedChange={onExpandedChange} />
        <NavItem current icon={<span />} label="Sessions" onSelect={onSelect} />
      </>
    );

    const toggle = screen.getByRole("button", { name: "Preview" });
    const disclosure = screen.getByRole("button", { name: "Details" });
    const nav = screen.getByRole("button", { name: "Sessions" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(disclosure.getAttribute("aria-controls")).toBe("details");
    expect(nav.getAttribute("aria-current")).toBe("page");
    expect(nav.textContent).toBe("Sessions");
    expect(nav.querySelector(".pevo-navItemMarker")).toBeNull();
    fireEvent.click(toggle);
    fireEvent.click(disclosure);
    fireEvent.click(nav);
    expect(onPressedChange).toHaveBeenCalledWith(true);
    expect(onExpandedChange).toHaveBeenCalledWith(true);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders external actions as safe links", () => {
    render(<ActionLink external href="https://example.com">Open documentation</ActionLink>);
    const link = screen.getByRole("link", { name: "Open documentation" });
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  });

  it("uses a radiogroup and arrow keys for mutually exclusive values", () => {
    const onValueChange = vi.fn();
    render(
      <SegmentedControl
        label="View mode"
        onValueChange={onValueChange}
        options={[{ label: "Source", value: "source" }, { label: "Preview", value: "preview" }]}
        value="source"
      />
    );
    const source = screen.getByRole("radio", { name: "Source" });
    const preview = screen.getByRole("radio", { name: "Preview" });
    expect(source.getAttribute("aria-checked")).toBe("true");
    expect(source.tabIndex).toBe(0);
    expect(preview.tabIndex).toBe(-1);
    fireEvent.keyDown(source, { key: "ArrowRight" });
    expect(onValueChange).toHaveBeenCalledWith("preview");
  });

  it("uses tab semantics for switching views", () => {
    const onValueChange = vi.fn();
    render(<Tabs label="Sections" onValueChange={onValueChange} options={[{ label: "General", value: "general" }, { label: "Advanced", value: "advanced" }]} value="general" />);
    const general = screen.getByRole("tab", { name: "General" });
    expect(general.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(general, { key: "ArrowRight" });
    expect(onValueChange).toHaveBeenCalledWith("advanced");
  });
});

describe("FormField", () => {
  it("wires label, hint, error, and input ARIA attributes", () => {
    render(
      <FormField error="Required" hint="Use a stable id" label="Provider id">
        <input />
      </FormField>
    );

    const input = screen.getByLabelText("Provider id");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(input.getAttribute("aria-describedby")).toContain("hint");
    expect(input.getAttribute("aria-describedby")).toContain("error");
    expect(screen.getByRole("alert").textContent).toBe("Required");
  });
});

describe("CreatePanel", () => {
  it("renders header, description, body, footer, and close action", () => {
    const onClose = vi.fn();
    render(
      <CreatePanel description="Configure a resource" footer={<button>Save</button>} layout="dialog" onClose={onClose} title="Add resource">
        <div>Fields</div>
      </CreatePanel>
    );

    expect(screen.getByRole("dialog", { name: "Add resource" })).toBeTruthy();
    expect(screen.getByText("Configure a resource")).toBeTruthy();
    expect(screen.getByText("Fields")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Save" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });
});

describe("Switch", () => {
  it("renders unchecked and checked ARIA state", () => {
    const { rerender } = render(<Switch checked={false} label="Debug" />);

    expect(screen.getByRole("switch", { name: "Debug" }).getAttribute("aria-checked")).toBe("false");

    rerender(<Switch checked label="Debug" />);
    expect(screen.getByRole("switch", { name: "Debug" }).getAttribute("aria-checked")).toBe("true");
  });

  it("calls onCheckedChange with the next checked value", () => {
    const onCheckedChange = vi.fn();
    render(<Switch checked={false} label="Debug" onCheckedChange={onCheckedChange} />);

    fireEvent.click(screen.getByRole("switch", { name: "Debug" }));

    expect(onCheckedChange).toHaveBeenCalledWith(true);
  });

  it("does not fire when disabled or pending", () => {
    const onDisabledChange = vi.fn();
    const onPendingChange = vi.fn();
    render(
      <>
        <Switch checked={false} disabled label="Disabled switch" onCheckedChange={onDisabledChange} />
        <Switch checked pending label="Pending switch" onCheckedChange={onPendingChange} />
      </>
    );

    fireEvent.click(screen.getByRole("switch", { name: "Disabled switch" }));
    fireEvent.click(screen.getByRole("switch", { name: "Pending switch" }));

    expect(onDisabledChange).not.toHaveBeenCalled();
    expect(onPendingChange).not.toHaveBeenCalled();
  });

  it("keeps the setting name as its accessible label when visual copy is hidden", () => {
    render(<Switch checked={false} label="Deploy" showLabel={false} />);

    expect(screen.getByRole("switch", { name: "Deploy" })).toBeTruthy();
    expect(screen.queryByText("Deploy")).toBeNull();
  });
});
