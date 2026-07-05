// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { gatewayMock } from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench capabilities management", () => {
  it("opens the top-level Capabilities view and composes domain tabs", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/list")).toBe(true);
    });
    expect(within(region).getByRole("tab", { name: "Skills" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "Plugins" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "MCP" })).toBeTruthy();
    expect(within(region).getByRole("tab", { name: "Tools" })).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Skill review" })).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Prompt" })).toBeNull();
    expect(within(region).queryByText("Prompt")).toBeNull();
    expect(within(region).queryByRole("button", { name: "All" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Enabled" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Disabled" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Collision" })).toBeNull();

    fireEvent.change(within(region).getByLabelText("Search Skills"), { target: { value: "bitmap" } });
    expect(within(region).queryByRole("button", { name: "Skill review" })).toBeNull();
    expect(within(region).getByRole("button", { name: "Skill imagegen" })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "Plugins" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "plugin/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /writer-kit/i })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "MCP" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "mcp/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /docs/i })).toBeTruthy();
    expect(within(region).getByRole("button", { name: "Test" })).toBeTruthy();

    fireEvent.click(within(region).getByRole("tab", { name: "Tools" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "tool/list")).toBe(true);
    });
    expect(await within(region).findByRole("button", { name: /web/i })).toBeTruthy();
    expect(within(region).queryByRole("tab", { name: "All" })).toBeNull();
  });

  it("keeps disabled skills visible and can re-enable them", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    const deploy = await within(region).findByRole("button", { name: "Skill deploy" });
    expect(deploy.textContent).not.toContain("Disabled");
    expect(deploy.textContent).toContain("Project");
    expect(deploy.textContent).not.toContain("project");
    expect(deploy.textContent).toContain("Setup Needed");

    fireEvent.click(deploy);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/read" && JSON.stringify(entry.params).includes("deploy"))).toBe(true);
    });
    expect(within(region).getByText("DEPLOY_TOKEN")).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Enable deploy" })).toBeNull();

    const enable = within(region).getByRole("switch", { name: "Enable deploy" });
    expect(enable.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(enable);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/setEnabled" && JSON.stringify(entry.params).includes("\"enabled\":true"))).toBe(true);
    });
    await waitFor(() => {
      expect(within(region).getByRole("switch", { name: "Disable deploy" }).getAttribute("aria-checked")).toBe("true");
    });
  });

  it("toggles plugin and MCP enablement from row switches", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(within(region).getByRole("tab", { name: "Plugins" }));
    const pluginSwitch = await within(region).findByRole("switch", { name: "Disable writer-kit" });
    expect(pluginSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(pluginSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "plugin/setEnabled",
        params: expect.objectContaining({
          selector: "local:/plugins/writer-kit",
          enabled: false
        })
      });
    });

    fireEvent.click(within(region).getByRole("tab", { name: "MCP" }));
    const mcpSwitch = await within(region).findByRole("switch", { name: "Disable docs" });
    expect(mcpSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(mcpSwitch);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "mcp/setEnabled",
        params: expect.objectContaining({
          name: "docs",
          enabled: false
        })
      });
    });
  });

  it("keeps coding-core tools read-only while web remains mode-configurable", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(within(region).getByRole("tab", { name: "Tools" }));
    const codingCore = await within(region).findByRole("button", { name: /coding-core/i });
    fireEvent.click(codingCore);

    const toolMutationsBefore = gatewayMock.requestLog.filter((entry) => entry.method === "tool/setEnabled").length;
    expect(within(region).queryByRole("button", { name: /^Default / })).toBeNull();
    expect(within(region).queryByRole("button", { name: /^Plan / })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Remove" })).toBeNull();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "tool/setEnabled").length).toBe(toolMutationsBefore);

    fireEvent.click(within(region).getByRole("button", { name: /web/i }));
    const defaultToggle = await within(region).findByRole("button", { name: "Default On" });
    expect(within(region).getByRole("button", { name: "Plan Off" })).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Remove" })).toBeNull();

    fireEvent.click(defaultToggle);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "tool/setEnabled",
        params: expect.objectContaining({
          name: "web",
          mode: "default",
          enabled: true
        })
      });
    });

    expect(within(region).queryByLabelText("Toolset name")).toBeNull();
    fireEvent.click(within(region).getByRole("button", { name: "Create toolset" }));
    fireEvent.change(await within(region).findByLabelText("Toolset name"), { target: { value: "coding-core" } });
    expect((within(region).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("searches skills and reads selected skill details", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await within(region).findByRole("button", { name: "Skill review" });
    fireEvent.change(within(region).getByLabelText("Search Skills"), { target: { value: "deploy" } });
    expect(within(region).getByRole("button", { name: "Skill deploy" })).toBeTruthy();
    expect(within(region).queryByRole("button", { name: "Skill review" })).toBeNull();

    fireEvent.click(within(region).getByRole("button", { name: "Skill deploy" }));
    await waitFor(() => {
      const read = [...gatewayMock.requestLog].reverse().find((entry) => entry.method === "skill/read");
      expect(JSON.stringify(read?.params)).toContain("/tmp/project/.psychevo/skills/deploy/SKILL.md");
    });
    const preview = within(region).getByLabelText("SKILL.md preview");
    await waitFor(() => {
      expect(within(preview).getByRole("heading", { name: "deploy workflow" })).toBeTruthy();
    });
    const table = within(preview).getByRole("table", { name: "YAML frontmatter" });
    expect(within(table).getByText("description")).toBeTruthy();
    expect(within(table).getByText("Deploy with release checks")).toBeTruthy();
    expect(within(table).getByText("Bash")).toBeTruthy();
    expect(within(preview).getByText("Follow the deploy workflow.")).toBeTruthy();
    fireEvent.click(within(preview).getByRole("button", { name: "Copy SKILL.md" }));
    await waitFor(() => {
      expect(gatewayMock.clipboardWriteLog[gatewayMock.clipboardWriteLog.length - 1]).toBe(
        "---\nname: deploy\ndescription: Deploy with release checks\nallowed-tools:\n  - Bash\n  - Read\n---\n# deploy workflow\n\nFollow the deploy workflow.\n\n- Confirm prerequisites\n\n```sh\npevo deploy\n```"
      );
    });
  });

  it("hides prompt and empty fields from skill details", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    fireEvent.click(await within(region).findByRole("button", { name: "Skill imagegen" }));
    const detail = within(region).getByLabelText("Skills detail");
    const preview = within(detail).getByLabelText("SKILL.md preview");
    const body = detail.querySelector(".skillDetailBody");

    await waitFor(() => {
      expect(within(preview).getByRole("heading", { name: "imagegen workflow" })).toBeTruthy();
    });
    expect(body?.children[0]?.classList.contains("skillDetailSummary")).toBe(true);
    expect(body?.children[1]).toBe(preview);
    expect(within(detail).queryByText("Prompt Visible")).toBeNull();
    expect(within(detail).queryByText("Tags")).toBeNull();
    expect(within(detail).queryByText("Missing Env")).toBeNull();
    expect(within(detail).queryByText("Missing Credentials")).toBeNull();
    expect(within(detail).queryByText("Tools")).toBeNull();
    expect(within(detail).queryByText("Toolsets")).toBeNull();
    expect(within(detail).queryByText("Linked Files")).toBeNull();
    expect(within(detail).queryByText("None")).toBeNull();
  });

  it("hides redundant skill entry file paths and shows non-standard entrypoints", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });

    await within(region).findByText("/tmp/profile/skills/review");
    expect(within(region).queryByText("Location")).toBeNull();
    expect(within(region).queryByText("Entrypoint")).toBeNull();

    fireEvent.click(within(region).getByRole("button", { name: "Skill root-note" }));
    await waitFor(() => {
      expect(within(region).getByText("Entrypoint")).toBeTruthy();
    });
    expect(within(region).getByText("/tmp/profile/skills/root-note.md")).toBeTruthy();
    expect(within(region).getByText("/tmp/profile/skills")).toBeTruthy();
  });

  it("requires confirmation before sending a force skill install", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const region = await screen.findByRole("region", { name: "Capabilities" });
    expect(within(region).queryByLabelText("Skill source")).toBeNull();
    fireEvent.click(within(region).getByRole("button", { name: "Install skill" }));
    fireEvent.change(await within(region).findByLabelText("Skill source"), { target: { value: "/tmp/skills/review" } });
    fireEvent.click(within(region).getByLabelText("Force"));

    fireEvent.click(within(region).getByRole("button", { name: "Install" }));
    expect(confirm).toHaveBeenCalledWith("Install skill with force?");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/install")).toBe(false);

    fireEvent.click(within(region).getByRole("button", { name: "Install" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "skill/install" && JSON.stringify(entry.params).includes("\"force\":true"))).toBe(true);
    });
    confirm.mockRestore();
  });
});
