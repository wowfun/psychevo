// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionButton, CreatePanel, FormField, Switch } from "./primitives";

afterEach(() => {
  cleanup();
});

describe("ActionButton", () => {
  it("defaults to a button type and supports variants, active state, and busy state", () => {
    render(
      <ActionButton active busy icon={<span data-testid="icon" />} variant="primary">
        Save
      </ActionButton>
    );

    const button = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
    expect(button.type).toBe("button");
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("aria-busy")).toBe("true");
    expect(button.getAttribute("aria-pressed")).toBe("true");
    expect(button.className).toContain("pevo-actionButton--primary");
    expect(screen.getByTestId("icon")).toBeTruthy();
  });

  it("supports icon-only accessible labels", () => {
    render(
      <ActionButton ariaLabel="Add backend" icon={<span />} iconOnly tooltip="Add backend">
        Add backend
      </ActionButton>
    );

    const button = screen.getByRole("button", { name: "Add backend" });
    expect(button.getAttribute("title")).toBe("Add backend");
    expect(screen.getByText("Add backend").className).toContain("pevo-srOnly");
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

  it("supports aria-label-only usage", () => {
    render(<Switch ariaLabel="Enable deploy" checked={false} label="Deploy" showLabel={false} />);

    expect(screen.getByRole("switch", { name: "Enable deploy" })).toBeTruthy();
    expect(screen.queryByText("Deploy")).toBeNull();
  });
});
