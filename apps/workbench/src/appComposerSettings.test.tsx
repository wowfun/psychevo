// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  agentRecord,
  commandItem,
  deferred,
  gatewayMock,
  observabilityResult,
  openAgentRuntimePopover,
  openRuntimeProfilePopover,
  selectMainAgent,
  selectRuntime,
  sessionSummary,
  workspaceDiffAction
} from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench settings and backend controls", () => {
  it("unmounts hidden left sidebar sections when collapsed", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Collapse left sidebar" }));

    expect(screen.getByRole("button", { name: "New Session" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Search" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Artifacts" })).toBeNull();
    expect(screen.queryByText("Pinned")).toBeNull();
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
  });

  it("shows an explicit Settings return action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    expect(settingsRegion).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Appearance" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("heading", { name: "Appearance" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Archived sessions" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Usage" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Models" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Slash Commands" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Debug" })).toBeTruthy();
    expect(within(settingsRegion).queryByRole("button", { name: "Agents" })).toBeNull();
    expect(within(settingsRegion).getByRole("button", { name: "Channels" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Dark" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Light" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Warm" })).toBeTruthy();
    for (const removed of ["General", "Session", "Session history", "Commands", "Integrations", "Diagnostics"]) {
      expect(within(settingsRegion).queryByRole("button", { name: removed })).toBeNull();
    }
    expect(within(settingsRegion).getByRole("searchbox", { name: "Search settings" })).toBeTruthy();
    expect(within(settingsRegion).queryByText("/tmp/project")).toBeNull();
    expect(within(settingsRegion).queryByRole("heading", { name: "Settings" })).toBeNull();
    expect(within(settingsRegion).queryByRole("button", { name: "Back to transcript" })).toBeNull();

    const backButton = within(settingsRegion).getByRole("button", { name: "Back to app" });

    fireEvent.click(backButton);
    expect(await screen.findByRole("region", { name: "Transcript" })).toBeTruthy();
  });

  it("shows the Debug switch without repeating its on/off state as text", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Debug" }));

    const debugSwitch = within(settingsRegion).getByRole("switch", { name: "Show debug tab" });
    const debugRow = debugSwitch.closest(".settingsRow") as HTMLElement;
    expect(debugRow).toBeTruthy();
    expect(within(debugRow).queryByText(/^On$/)).toBeNull();
    expect(within(debugRow).queryByText(/^Off$/)).toBeNull();
  });

  it("manages profile slash aliases and shortcuts from Settings", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Slash Commands" }));

    const slashPanel = await within(settingsRegion).findByRole("region", { name: "Slash Commands" });
    expect(within(slashPanel).getByText("Profile Slash Commands")).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "slash/settings/read")).toBe(true);
    });

    const commandListRefreshesBefore = gatewayMock.requestLog.filter((entry) => entry.method === "command/list").length;
    fireEvent.change(within(slashPanel).getByLabelText("Alias"), { target: { value: "/st" } });
    fireEvent.change(within(slashPanel).getAllByLabelText("Target slash line")[0]!, { target: { value: "/status" } });
    fireEvent.click(within(slashPanel).getByRole("button", { name: "Save alias" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "slash/settings/update",
        params: expect.objectContaining({
          aliases: [expect.objectContaining({ alias: "/st", target: "/status" })]
        })
      });
    });
    expect(await within(slashPanel).findByText("/st")).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "command/list").length).toBeGreaterThan(commandListRefreshesBefore);
    });

    fireEvent.click(within(slashPanel).getByRole("button", { name: "Edit alias /st" }));
    fireEvent.change(within(slashPanel).getByLabelText("Alias"), { target: { value: "/stat" } });
    fireEvent.click(within(slashPanel).getByRole("button", { name: "Save alias" }));
    expect(await within(slashPanel).findByText("/stat")).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.slashSettings.aliases).toEqual([
        expect.objectContaining({ alias: "/stat", target: "/status" })
      ]);
    });

    fireEvent.click(within(slashPanel).getByRole("button", { name: "Delete alias /stat" }));
    await waitFor(() => {
      expect(gatewayMock.slashSettings.aliases).toEqual([]);
    });
    expect(within(slashPanel).queryByText("/stat")).toBeNull();

    fireEvent.change(within(slashPanel).getByLabelText("Shortcut"), { target: { value: "<leader>s" } });
    fireEvent.change(within(slashPanel).getAllByLabelText("Target slash line")[1]!, { target: { value: "/status" } });
    fireEvent.click(within(slashPanel).getByRole("button", { name: "Save shortcut" }));
    expect(await within(slashPanel).findByText("<leader>s")).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.slashSettings.keybinds).toEqual([
        expect.objectContaining({ shortcut: "<leader>s", target: "/status" })
      ]);
    });

    fireEvent.click(within(slashPanel).getByRole("button", { name: "Delete shortcut <leader>s" }));
    await waitFor(() => {
      expect(gatewayMock.slashSettings.keybinds).toEqual([]);
    });
  });

  it("configures providers and auxiliary models in Settings Models", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Models" }));

    const modelsPanel = await within(settingsRegion).findByRole("region", { name: "Models" });
    expect(within(modelsPanel).getByText("OpenCode Zen")).toBeTruthy();
    expect(within(modelsPanel).getByText("Xiaomi Token Plan")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "model/settings/read")).toBe(true);

    const zenRow = within(modelsPanel).getByText("OpenCode Zen").closest(".modelProviderRow") as HTMLElement;
    fireEvent.click(within(zenRow).getByRole("button", { name: "Edit" }));
    fireEvent.click(within(modelsPanel).getByRole("button", { name: "Fetch models" }));
    expect(await within(modelsPanel).findByText("OpenCode Zen catalog updated")).toBeTruthy();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "model/provider/catalog",
      params: expect.objectContaining({
        providerId: "opencode-zen",
        refresh: true
      })
    });

    const defaultRow = within(modelsPanel).getByText("Default model").closest(".modelAssignmentRow") as HTMLElement;
    fireEvent.click(within(defaultRow).getByRole("button", { name: "Default model" }));
    const defaultPicker = within(defaultRow).getByRole("dialog", { name: "Default model and reasoning" });
    expect(within(defaultPicker).getByRole("searchbox", { name: "Default model filter" })).toBeTruthy();
    const freeDefaultRow = within(defaultPicker).getByRole("radio", { name: /mimo-v2\.5-free/ });
    expect(freeDefaultRow.getAttribute("data-model-free")).toBe("true");
    expect(freeDefaultRow.querySelector(".modelReasoningFreeBadge")?.textContent).toBe("Free");
    const paidDefaultRow = within(defaultPicker).getByRole("radio", { name: /deepseek-v4-pro/ });
    expect(paidDefaultRow.getAttribute("data-model-free")).toBeNull();
    expect(paidDefaultRow.querySelector(".modelReasoningFreeBadge")).toBeNull();
    fireEvent.click(freeDefaultRow);
    fireEvent.click(within(defaultPicker).getByRole("radio", { name: "High" }));
    fireEvent.keyDown(defaultPicker, { key: "Escape" });
    expect(within(modelsPanel).getByText(/free models may route data/)).toBeTruthy();
    expect(within(defaultRow).queryByRole("textbox")).toBeNull();
    const settingsReadCountBeforeDefaultSave = gatewayMock.requestLog.filter((entry) => entry.method === "settings/read").length;
    fireEvent.click(within(defaultRow).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "model/assignment/set",
        params: expect.objectContaining({
          target: "default",
          provider: "opencode-zen",
          model: "mimo-v2.5-free",
          reasoningEffort: "high"
        })
      });
    });
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "settings/read").length).toBeGreaterThan(settingsReadCountBeforeDefaultSave);
    });

    const titleRow = within(modelsPanel).getByText("Title generation").closest(".modelAssignmentRow") as HTMLElement;
    fireEvent.click(within(titleRow).getByRole("button", { name: "Title generation" }));
    const titlePicker = within(titleRow).getByRole("dialog", { name: "Title generation and reasoning" });
    fireEvent.click(within(titlePicker).getByRole("radio", { name: /mimo-v2\.5-free/ }));
    fireEvent.click(within(titlePicker).getByRole("radio", { name: "Low" }));
    fireEvent.click(within(titleRow).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "model/assignment/set",
        params: expect.objectContaining({
          target: "auxiliary",
          task: "title_generation",
          provider: "opencode-zen",
          model: "mimo-v2.5-free",
          reasoningEffort: "low"
        })
      });
    });

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    const modelSelect = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(modelSelect.getAttribute("title")).toBe("opencode-zen/mimo-v2.5-free / Default");
    });
    const composerPicker = await openComposerModelPicker();
    expect(within(composerPicker).getByRole("radio", { name: /mimo-v2\.5-free/ })).toBeTruthy();
    fireEvent.keyDown(composerPicker, { key: "Escape" });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "use the saved default" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "use the saved default" }],
          target: { agentRef: null, runtimeProfileRef: "native" },
          turnOverrides: {}
        })
      });
    });
  });

  it("keeps the scoped composer model after saving a different global default", async () => {
    gatewayMock.model = "openai/gpt-4o";
    gatewayMock.modelSettings = {
      ...gatewayMock.modelSettings,
      defaultModel: "openai/gpt-4o"
    };
    gatewayMock.modelOverride = "xiaomi/xiaomi-token-high";

    render(<App />);

    const initialModelSelect = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(initialModelSelect.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    });

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Models" }));
    const modelsPanel = await within(settingsRegion).findByRole("region", { name: "Models" });
    const zenRow = within(modelsPanel).getByText("OpenCode Zen").closest(".modelProviderRow") as HTMLElement;
    fireEvent.click(within(zenRow).getByRole("button", { name: "Edit" }));
    fireEvent.click(within(modelsPanel).getByRole("button", { name: "Fetch models" }));
    expect(await within(modelsPanel).findByText("OpenCode Zen catalog updated")).toBeTruthy();

    const defaultRow = within(modelsPanel).getByText("Default model").closest(".modelAssignmentRow") as HTMLElement;
    fireEvent.click(within(defaultRow).getByRole("button", { name: "Default model" }));
    const defaultPicker = within(defaultRow).getByRole("dialog", { name: "Default model and reasoning" });
    fireEvent.click(within(defaultPicker).getByRole("radio", { name: /mimo-v2\.5-free/ }));
    fireEvent.keyDown(defaultPicker, { key: "Escape" });
    fireEvent.click(within(defaultRow).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "model/assignment/set",
        params: expect.objectContaining({
          target: "default",
          provider: "opencode-zen",
          model: "mimo-v2.5-free"
        })
      });
    });

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    const modelSelect = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(modelSelect.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "keep scoped model" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "keep scoped model" }],
          target: { agentRef: null, runtimeProfileRef: "native" },
          turnOverrides: {}
        })
      });
    });
  });

  it("shows the global Settings default separately from the current effective composer model", async () => {
    gatewayMock.model = "xiaomi/xiaomi-token-high";
    gatewayMock.modelSettings = {
      ...gatewayMock.modelSettings,
      defaultModel: "opencode-zen/big-pickle",
      defaultReasoningEffort: "high",
      modelOptions: [
        ...(gatewayMock.modelSettings.modelOptions as Array<Record<string, unknown>>),
        { provider: "opencode-zen", id: "big-pickle", value: "opencode-zen/big-pickle", name: null, providerName: "OpenCode Zen", free: true, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
      ]
    };

    render(<App />);

    const composerModelSelect = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(composerModelSelect.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    });

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Models" }));
    const modelsPanel = await within(settingsRegion).findByRole("region", { name: "Models" });
    const defaultRow = within(modelsPanel).getByText("Default model").closest(".modelAssignmentRow") as HTMLElement;
    const defaultButton = within(defaultRow).getByRole("button", { name: "Default model" });

    await waitFor(() => {
      expect(defaultButton.textContent).toContain("big-pickle High");
    });
    expect(defaultButton.textContent).not.toContain("xiaomi-token-high");
    expect(defaultButton.getAttribute("title")).toBe("opencode-zen/big-pickle / High");

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    const currentComposerModelSelect = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(currentComposerModelSelect.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    });
  });

  it("loads all-history usage summaries in Settings", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Usage" }));

    const usagePanel = await within(settingsRegion).findByRole("region", { name: "Usage" });
    expect(await within(usagePanel).findByText("All time")).toBeTruthy();
    expect(within(usagePanel).getByText("Last 30 days")).toBeTruthy();
    expect(within(usagePanel).getByText("Last 7 days")).toBeTruthy();
    expect(within(usagePanel).getByText("Token activity")).toBeTruthy();
    const heatmap = within(usagePanel).getByRole("region", { name: "Token activity" });
    for (const level of ["1", "2", "3", "4"]) {
      expect(heatmap.querySelectorAll(`.usageHeatmapCell[data-level="${level}"]`).length).toBeGreaterThan(0);
    }
    expect(within(usagePanel).getByRole("button", { name: "Refresh usage" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => (
      entry.method === "usage/read"
      && (entry.params as { activityDays?: number }).activityDays === 365
    ))).toBe(true);
  });

  it("does not expose Agent management from Settings", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });

    expect(within(settingsRegion).queryByRole("button", { name: "Agents" })).toBeNull();
    expect(within(settingsRegion).queryByRole("region", { name: "Agents" })).toBeNull();
    expect(within(settingsRegion).queryByRole("button", { name: "Add ACP backend" })).toBeNull();
    expect(within(settingsRegion).queryByText("Profile ACP Backends")).toBeNull();
    expect(within(settingsRegion).queryByText("Translate user messages")).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Agent" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Model" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Permission mode" })).toBeNull();
  });

  it("shows Channels as Settings rows with switches and an independent detail page", async () => {
    gatewayMock.channelRecords = gatewayMock.channelRecords.map((channel) => (
      channel.id === "release"
        ? { ...channel, model: "custom/current-model" }
        : channel
    ));
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));

    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    expect(within(channelsPanel).getByText("Connected Channels")).toBeTruthy();
    expect(within(channelsPanel).queryByRole("button", { name: "All" })).toBeNull();
    expect(within(channelsPanel).queryByRole("button", { name: "Enabled" })).toBeNull();
    expect(within(channelsPanel).queryByText("Enabled")).toBeNull();
    expect(within(channelsPanel).queryByText("Disabled")).toBeNull();
    expect(within(channelsPanel).queryByRole("button", { name: "Needs setup" })).toBeNull();
    expect(within(channelsPanel).getByText("Release Bot")).toBeTruthy();
    expect(within(channelsPanel).getAllByText("ready").length).toBeGreaterThan(0);
    expect(within(channelsPanel).getAllByText("Credential present").length).toBeGreaterThan(0);
    expect(within(channelsPanel).getAllByText("Allowlist present").length).toBeGreaterThan(0);

    fireEvent.click(within(channelsPanel).getByRole("switch", { name: "Disable release" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "channel/enable",
        params: expect.objectContaining({
          id: "release",
          enabled: false
        })
      });
    });

    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Test release" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "channel/doctor",
        params: expect.objectContaining({ id: "release", live: false })
      });
    });

    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Settings release" }));
    const detailPage = await within(settingsRegion).findByRole("region", { name: "Channel settings" });
    expect(within(detailPage).getByRole("button", { name: "Back to Channels" })).toBeTruthy();
    expect(within(detailPage).getByRole("heading", { name: "Connection" })).toBeTruthy();
    expect(within(detailPage).getByRole("heading", { name: "Access control" })).toBeTruthy();
    expect(within(detailPage).getByRole("heading", { name: "Runtime settings" })).toBeTruthy();
    expect(within(detailPage).getByText("Advanced diagnostics")).toBeTruthy();
    expect(within(detailPage).queryByRole("heading", { name: "Credentials" })).toBeNull();
    expect(within(detailPage).queryByRole("heading", { name: "Runtime runner diagnostics" })).toBeNull();
    expect(within(detailPage).getByRole("heading", { name: "Danger zone" })).toBeTruthy();
    const detailForm = detailPage.querySelector(".channelDetailsForm");
    expect(detailForm?.classList.contains("channelDetailsGroups")).toBe(false);
    expect(Array.from(detailForm?.children ?? []).filter((child) => child.classList.contains("channelDetailSection")).length).toBe(3);
    expect(within(detailPage).getByText("Allowed callers")).toBeTruthy();
    expect(within(detailPage).queryByText("Connection identity")).toBeNull();
    expect(within(detailPage).queryByText("Connected Channels")).toBeNull();
    expect(within(detailPage).queryByRole("switch", { name: "Enable release on save" })).toBeNull();
    expect(within(detailPage).queryByRole("switch", { name: "Disable release on save" })).toBeNull();
    expect(within(detailPage).queryByRole("button", { name: "Test release" })).toBeNull();
    expect(within(detailPage).queryByRole("button", { name: "Cancel" })).toBeNull();
    expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    expect(await within(detailPage).findByRole("option", { name: "custom/current-model (current, unavailable)" })).toBeTruthy();
    const workspacePreset = within(detailPage).getByRole("combobox", { name: "Channel workspace preset" });
    expect(workspacePreset).toBeTruthy();
    expect(within(workspacePreset).getByRole("option", { name: "Profile default" })).toBeTruthy();
    expect(within(workspacePreset).getByRole("option", { name: "project - /tmp/project" })).toBeTruthy();
    expect(within(detailPage).getByText("Changing workspace starts a fresh channel thread on the next message. Current running work is not interrupted.")).toBeTruthy();

    const labelInput = within(detailPage).getByRole("textbox", { name: "Channel label" }) as HTMLInputElement;
    fireEvent.change(labelInput, { target: { value: "Release Ops" } });
    expect(within(detailPage).queryByText("Unsaved changes")).toBeNull();
    expect(within(detailPage).getByRole("button", { name: "Cancel" })).toBeTruthy();
    expect(within(detailPage).getAllByRole("button", { name: "Cancel" })).toHaveLength(1);
    expect(within(detailPage).getAllByRole("button", { name: "Save" })).toHaveLength(1);
    expect((within(detailPage).getAllByRole("button", { name: "Save" })[0] as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(within(detailPage).getByRole("button", { name: "Back to Channels" }));
    expect(within(detailPage).getByText("Discard unsaved changes?")).toBeTruthy();
    fireEvent.click(within(detailPage).getByRole("button", { name: "Keep editing" }));
    expect(within(detailPage).queryByText("Discard unsaved changes?")).toBeNull();

    fireEvent.click(within(detailPage).getByRole("button", { name: "Cancel" }));
    expect((within(detailPage).getByRole("textbox", { name: "Channel label" }) as HTMLInputElement).value).toBe("Release Bot");
    expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(within(detailPage).getByRole("textbox", { name: "Channel label" }), { target: { value: "Release Ops" } });
    fireEvent.change(within(detailPage).getByRole("textbox", { name: "Allowed direct users" }), {
      target: { value: "alice, bob\nalice" }
    });
    fireEvent.change(within(detailPage).getByRole("textbox", { name: "Allowed groups" }), {
      target: { value: "team\nteam, ops" }
    });
    fireEvent.click(within(detailPage).getByRole("checkbox", { name: "Require mention in groups" }));
    fireEvent.change(within(detailPage).getByRole("combobox", { name: "Channel model" }), {
      target: { value: "openai/gpt-4o" }
    });
    fireEvent.change(within(detailPage).getByRole("textbox", { name: "Channel workspace" }), {
      target: { value: "/tmp/channel-workspace" }
    });
    const bypassPermissions = await within(detailPage).findByRole("button", { name: "Bypass permissions" }) as HTMLButtonElement;
    await waitFor(() => expect(bypassPermissions.disabled).toBe(false));
    fireEvent.click(bypassPermissions);
    fireEvent.click(within(detailPage).getByText("Advanced diagnostics"));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "channel/source/list")).toBe(true);
    });
    expect(within(detailPage).getByText("Remote lanes")).toBeTruthy();
    expect(within(detailPage).getByText("Channel lane")).toBeTruthy();
    expect(within(detailPage).getByText("/tmp/project")).toBeTruthy();
    expect(within(detailPage).getByRole("option", { name: "OpenCode (ACP)" })).toBeTruthy();
    fireEvent.change(within(detailPage).getByRole("combobox", { name: "Channel Runtime Profile" }), {
      target: { value: "opencode" }
    });
    const acpModel = await within(detailPage).findByRole("combobox", { name: "Channel model" }) as HTMLSelectElement;
    await waitFor(() => {
      expect(within(acpModel).getByRole("option", { name: "openai/gpt-fixture" })).toBeTruthy();
      expect(acpModel.disabled).toBe(false);
    });
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/context/read",
      params: expect.objectContaining({
        threadId: null,
        target: { agentRef: "opencode", runtimeProfileRef: "opencode" },
        scope: expect.objectContaining({
          source: expect.objectContaining({
            rawId: "channel-settings:release",
            lifetime: "invocation"
          })
        })
      })
    });
    expect(within(acpModel).getByRole("option", { name: "openai/gpt-4o (current, unavailable)" })).toBeTruthy();
    fireEvent.change(acpModel, { target: { value: "openai/gpt-fixture" } });
    expect(within(detailPage).queryByRole("group", { name: "Permission mode" })).toBeNull();
    expect(within(detailPage).getByLabelText("Runtime Profile safety policy").textContent).toContain("OpenCode");
    expect(within(detailPage).queryByText("Uses runtime default")).toBeNull();
    expect(within(detailPage).getByText(/Permission mode bypassPermissions is not an authoritative choice/)).toBeTruthy();
    fireEvent.click(within(detailPage).getByRole("button", { name: "Use profile safety policy" }));
    fireEvent.change(within(detailPage).getByRole("textbox", { name: "Credential env" }), {
      target: { value: "" }
    });
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "channel/update")).toBe(true);
    });
    const updateEntry = gatewayMock.requestLog.find((entry) => entry.method === "channel/update");
    expect(updateEntry?.params).toEqual(expect.objectContaining({
      id: "release",
      label: "Release Ops",
      enabled: false,
      cwd: "/tmp/channel-workspace",
      runtimeRef: "opencode",
      model: "openai/gpt-fixture",
      permissionMode: "default",
      requireMention: false,
      credentialEnv: "",
      allowUsers: ["alice", "bob"],
      allowGroups: ["team", "ops"]
    }));
    expect(updateEntry?.params).not.toHaveProperty("accountEnv");
    expect(updateEntry?.params).not.toHaveProperty("baseUrlEnv");
    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    });
    expect(within(detailPage).getByText("Next message will start in the new workspace.")).toBeTruthy();

    fireEvent.click(within(detailPage).getByRole("button", { name: "Remove channel" }));
    expect(within(detailPage).getByRole("button", { name: "Confirm remove" })).toBeTruthy();
    fireEvent.click(within(detailPage).getByRole("button", { name: "Confirm remove" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "channel/delete",
        params: expect.objectContaining({ id: "release" })
      });
    });
    const listAgain = await within(settingsRegion).findByRole("region", { name: "Channels" });
    expect(within(listAgain).queryByText("Release Ops")).toBeNull();
    expect(within(listAgain).queryByRole("tab", { name: "Feishu" })).toBeNull();
    fireEvent.click(within(listAgain).getByRole("button", { name: "Set up channel" }));
    fireEvent.click(within(listAgain).getByRole("tab", { name: "Feishu" }));
    expect(within(listAgain).getByText("FEISHU_APP_ID")).toBeTruthy();
    fireEvent.click(within(listAgain).getByRole("tab", { name: "WeChat" }));
    expect(within(listAgain).getByText("Generate a QR code, scan it with WeChat, then Psychevo saves the iLink token locally.")).toBeTruthy();

    vi.useFakeTimers({ now: new Date("2026-06-22T00:00:00Z") });
    try {
      fireEvent.click(within(listAgain).getByRole("button", { name: "Generate QR" }));
      await act(async () => {
        await Promise.resolve();
      });
      expect(gatewayMock.requestLog).toContainEqual({
        method: "channel/wechat-qr/start",
        params: expect.objectContaining({ id: "wechat", label: "WeChat" })
      });
      const qrRegion = within(listAgain).getByLabelText("WeChat QR code");
      const directQr = qrRegion.querySelector("img");
      expect(directQr?.getAttribute("src")).toBe("data:image/png;base64,wechat-qr-image");
      expect(within(listAgain).getByText("120s left")).toBeTruthy();
      act(() => {
        vi.advanceTimersByTime(1000);
      });
      expect(within(listAgain).getByText("119s left")).toBeTruthy();

      fireEvent.click(within(listAgain).getByRole("button", { name: "Check status" }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(gatewayMock.requestLog).toContainEqual({
        method: "channel/wechat-qr/poll",
        params: expect.objectContaining({ sessionId: "wechat-session", enable: true })
      });
      expect(within(listAgain).getByText("WeChat polling is starting")).toBeTruthy();
      expect(within(listAgain).getByText("WeChat credentials saved. Gateway is starting polling.")).toBeTruthy();
      expect(within(listAgain).queryByText("WeChat QR session not found")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("fails closed when an ACP model descriptor is not certified for Channels", async () => {
    gatewayMock.acpChannelModelSafe = false;
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Settings ops-lark" }));

    const detailPage = await within(settingsRegion).findByRole("region", { name: "Channel settings" });
    expect(await within(detailPage).findByText("Model is not certified for Channels.")).toBeTruthy();
    expect(within(detailPage).queryByRole("combobox", { name: "Channel model" })).toBeNull();
    expect(within(detailPage).getByText("Model control unavailable")).toBeTruthy();
    expect(within(detailPage).queryByText("Uses runtime default")).toBeNull();
  });

  it("keeps internal WeChat env names out of the default detail save surface", async () => {
    gatewayMock.channelRecords = [
      {
        id: "wechat",
        channel: "wechat",
        domain: "wechat",
        enabled: true,
        label: "WeChat Ops",
        transport: "polling",
        cwd: null,
        model: null,
        permissionMode: null,
        requireMention: true,
        credential: { env: "WECHAT_BOT_TOKEN", status: "present" },
        account: { env: "WECHAT_ACCOUNT_ID", status: "present" },
        baseUrl: { env: "WECHAT_ILINK_BASE_URL", status: "present" },
        appId: null,
        allowlist: { users: ["wx-user"], groups: [], status: "present" },
        runtimeStatus: "ready",
        runner: {
          state: "running",
          reason: "polling_empty",
          lastPollAtMs: Date.now(),
          lastHealthyPollAtMs: Date.now(),
          lastInboundAtMs: null,
          lastOutboundAtMs: null,
          lastIlinkErrcode: null,
          lastError: null
        }
      }
    ];
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Settings wechat" }));

    const wechatDetail = await within(settingsRegion).findByRole("region", { name: "Channel settings" });
    expect(within(wechatDetail).queryByText("Account env")).toBeNull();
    expect(within(wechatDetail).queryByText("Base URL env")).toBeNull();
    expect(within(wechatDetail).queryByText("WECHAT_ACCOUNT_ID")).toBeNull();
    expect(within(wechatDetail).queryByText("WECHAT_ILINK_BASE_URL")).toBeNull();
    expect(within(wechatDetail).getByText("Advanced diagnostics")).toBeTruthy();

    fireEvent.change(within(wechatDetail).getByRole("textbox", { name: "Channel label" }), { target: { value: "WeChat Ops Edited" } });
    fireEvent.click(within(wechatDetail).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "channel/update")).toBe(true);
    });
    const updateEntry = gatewayMock.requestLog.find((entry) => entry.method === "channel/update");
    expect(updateEntry?.params).toEqual(expect.objectContaining({
      id: "wechat",
      label: "WeChat Ops Edited"
    }));
    expect(updateEntry?.params).not.toHaveProperty("accountEnv");
    expect(updateEntry?.params).not.toHaveProperty("baseUrlEnv");
  });

  it("saves Channel workspace picker choices while keeping manual paths available", async () => {
    gatewayMock.browserWorkspaces = [
      {
        cwd: "/tmp/project",
        project: { cwd: "/tmp/project", label: "project", displayPath: "/tmp/project" },
        sessions: [],
        hiddenCount: 0,
        nextCursor: null
      },
      {
        cwd: "/tmp/recent-ops",
        project: { cwd: "/tmp/recent-ops", label: "recent-ops", displayPath: "/tmp/recent-ops" },
        sessions: [],
        hiddenCount: 0,
        nextCursor: null
      }
    ];
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Settings release" }));

    const detailPage = await within(settingsRegion).findByRole("region", { name: "Channel settings" });
    const workspacePreset = within(detailPage).getByRole("combobox", { name: "Channel workspace preset" }) as HTMLSelectElement;
    const workspaceInput = within(detailPage).getByRole("textbox", { name: "Channel workspace" }) as HTMLInputElement;
    expect(within(detailPage).getByRole("option", { name: "project - /tmp/project" })).toBeTruthy();
    expect(within(detailPage).getByRole("option", { name: "recent-ops - /tmp/recent-ops" })).toBeTruthy();

    fireEvent.change(workspacePreset, { target: { value: "/tmp/recent-ops" } });
    expect(workspaceInput.value).toBe("/tmp/recent-ops");
    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(1);
    });
    expect(within(detailPage).getByText("Next message will start in the new workspace.")).toBeTruthy();
    expect(gatewayMock.requestLog.find((entry) => entry.method === "channel/update")?.params).toEqual(expect.objectContaining({
      id: "release",
      cwd: "/tmp/recent-ops"
    }));

    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    });
    fireEvent.change(workspacePreset, { target: { value: "" } });
    expect(workspaceInput.value).toBe("");
    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(2);
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").at(-1)?.params).toEqual(expect.objectContaining({
      id: "release",
      cwd: ""
    }));

    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    });
    fireEvent.change(workspaceInput, { target: { value: "/tmp/manual-channel" } });
    expect(workspacePreset.value).toBe("__manual__");
    expect(within(detailPage).getByRole("option", { name: "Manual path" })).toBeTruthy();
    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(3);
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").at(-1)?.params).toEqual(expect.objectContaining({
      id: "release",
      cwd: "/tmp/manual-channel"
    }));
  });

  it("clears stale WeChat QR sessions instead of leaving a scannable expired code", async () => {
    gatewayMock.wechatQrPoll = () => {
      throw new Error("WeChat QR session not found");
    };
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Set up channel" }));
    fireEvent.click(within(channelsPanel).getByRole("tab", { name: "WeChat" }));
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Generate QR" }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(within(channelsPanel).getByLabelText("WeChat QR code").querySelector("img")).toBeTruthy();

    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Check status" }));
    await waitFor(() => {
      expect(within(channelsPanel).getByText(/expired, completed, or was created before the Gateway restarted/)).toBeTruthy();
    });
    expect(within(channelsPanel).getByLabelText("WeChat QR code").querySelector("img")).toBeNull();
    expect(within(channelsPanel).queryByText(/s left/)).toBeNull();
    expect((within(channelsPanel).getByRole("button", { name: "Check status" }) as HTMLButtonElement).disabled).toBe(true);
    expect(within(channelsPanel).getByRole("button", { name: "Generate again" })).toBeTruthy();
  });

  it("shows reconnect-first WeChat setup when the runner needs QR login", async () => {
    gatewayMock.channelRecords = [
      {
        id: "wechat",
        channel: "wechat",
        domain: "wechat",
        enabled: true,
        label: "WeChat",
        transport: "polling",
        cwd: null,
        model: null,
        permissionMode: null,
        requireMention: true,
        credential: { env: "WECHAT_BOT_TOKEN", status: "present" },
        allowlist: { users: ["wx-user"], groups: [], status: "present" },
        runtimeStatus: "ready",
        runner: {
          state: "blocked",
          reason: "needs_qr_login",
          lastPollAtMs: null,
          lastHealthyPollAtMs: null,
          lastInboundAtMs: null,
          lastOutboundAtMs: null,
          lastIlinkErrcode: -14,
          lastError: "WeChat iLink getupdates failed: needs_qr_login errcode=-14: session timeout"
        }
      }
    ];
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Set up channel" }));
    fireEvent.click(within(channelsPanel).getByRole("tab", { name: "WeChat" }));

    expect(within(channelsPanel).getByText("WeChat reconnect required")).toBeTruthy();
    expect(within(channelsPanel).queryByText("WeChat connected")).toBeNull();
    expect(within(channelsPanel).getByRole("button", { name: "Reconnect QR" })).toBeTruthy();
    expect(within(channelsPanel).getByText("needs_qr_login")).toBeTruthy();
  });

  it("shows a neutral WeChat setup state while fresh QR polling is starting", async () => {
    gatewayMock.channelRecords = [
      {
        id: "wechat",
        channel: "wechat",
        domain: "wechat",
        enabled: true,
        label: "WeChat",
        transport: "polling",
        cwd: null,
        model: null,
        permissionMode: null,
        requireMention: true,
        credential: { env: "WECHAT_BOT_TOKEN", status: "present" },
        allowlist: { users: ["wx-user"], groups: [], status: "present" },
        runtimeStatus: "ready",
        runner: {
          state: "running",
          reason: "qr_login_pending",
          lastPollAtMs: null,
          lastHealthyPollAtMs: null,
          lastInboundAtMs: null,
          lastOutboundAtMs: null,
          lastIlinkErrcode: -14,
          lastError: null
        }
      }
    ];
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Channels" }));
    const channelsPanel = await within(settingsRegion).findByRole("region", { name: "Channels" });
    fireEvent.click(within(channelsPanel).getByRole("button", { name: "Set up channel" }));
    fireEvent.click(within(channelsPanel).getByRole("tab", { name: "WeChat" }));

    expect(within(channelsPanel).getByText("WeChat polling is starting")).toBeTruthy();
    expect(within(channelsPanel).getByText("qr_login_pending")).toBeTruthy();
    expect(within(channelsPanel).queryByText("WeChat connected")).toBeNull();
    expect(within(channelsPanel).queryByText("WeChat reconnect required")).toBeNull();
  });

  it("shows archived sessions from Settings without turning the sidebar into an archive filter", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("active-thread", "Active session")];
    gatewayMock.archivedSessionSummaries = [sessionSummary("archived-thread", "Archived session")];

    render(<App />);

    expect(await screen.findByText("Active session")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Archived sessions" }));

    const archivedPanel = await within(settingsRegion).findByRole("region", { name: "Archived sessions" });
    expect(await within(archivedPanel).findByText("Archived session")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Active session")).toBeNull();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/list",
      params: expect.objectContaining({ archived: true })
    });
  });

  it("renders model and reasoning from the canonical Thread control descriptors", async () => {
    render(<App />);

    const model = await screen.findByRole("button", { name: "Model" });
    expect(model.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    fireEvent.click(model);
    const picker = await screen.findByRole("dialog", { name: "Model and reasoning" });
    expect(within(picker).getByRole("radiogroup", { name: "Model" }).querySelectorAll('[role="radio"]')).toHaveLength(3);
    expect(within(within(picker).getByRole("radiogroup", { name: "Model" })).getAllByRole("radio").map((option) => option.textContent)).toEqual([
      "xiaomi-token-high",
      "gpt-4o",
      "xiaomi-token-low"
    ]);
    const reasoning = within(picker).getByRole("radiogroup", { name: "Reasoning" });
    expect(within(reasoning).getByRole("radio", { name: "Default" }).getAttribute("aria-checked")).toBe("true");
    expect(within(reasoning).getAllByRole("radio").map((option) => option.textContent)).toEqual([
      "Default",
      "Low",
      "Medium",
      "High"
    ]);
  });

  it("preserves the Adapter-projected model choice order", async () => {
    render(<App />);

    const picker = await openComposerModelPicker();
    const models = within(picker).getByRole("radiogroup", { name: "Model" });
    expect(within(models).getAllByRole("radio").map((option) => option.textContent)).toEqual([
      "xiaomi-token-high",
      "gpt-4o",
      "xiaomi-token-low"
    ]);
  });

  it("routes composer model choices through Thread control receipts", async () => {
    render(<App />);

    let picker = await openComposerModelPicker();
    fireEvent.click(within(picker).getByRole("radio", { name: "gpt-4o" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/control/set",
        params: expect.objectContaining({
          controlId: "model",
          value: "openai/gpt-4o"
        })
      });
    });

    picker = await openComposerModelPicker();
    fireEvent.click(within(picker).getByRole("radio", { name: "xiaomi-token-low" }));
    await waitFor(() => expect(within(picker).queryByRole("radio", { name: "Medium" })).toBeTruthy());
    fireEvent.click(within(picker).getByRole("radio", { name: "Medium" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/control/set",
        params: expect.objectContaining({
          controlId: "reasoning",
          value: "medium"
        })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "model/state/set")).toBe(false);
  });

  it("adapts reasoning options when switching models", async () => {
    render(<App />);

    const picker = await openComposerModelPicker();
    fireEvent.click(within(picker).getByRole("radio", { name: "High" }));
    await waitFor(() => expect(within(picker).getByRole("radio", { name: "High" }).getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(within(picker).getByRole("radio", { name: "gpt-4o" }));

    await waitFor(() => {
      const reasoning = within(picker).getByRole("radiogroup", { name: "Reasoning" });
      expect(within(reasoning).getByRole("radio", { name: "Default" }).getAttribute("aria-checked")).toBe("true");
      expect(within(reasoning).queryByRole("radio", { name: "High" })).toBeNull();
    });
  });

  it("blocks prompt turns until a concrete provider-qualified model is selected", async () => {
    gatewayMock.model = null;
    gatewayMock.modelStatus = "unconfigured";

    render(<App />);

    const model = await screen.findByRole("button", { name: "Model" });
    expect(model.textContent).toBe("Select model");
    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello" } });
    const sendButton = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    expect(sendButton.disabled).toBe(true);
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    const picker = await openComposerModelPicker();
    fireEvent.click(within(picker).getByRole("radio", { name: "gpt-4o" }));
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "hello" }],
          target: { agentRef: null, runtimeProfileRef: "native" },
          turnOverrides: {}
        })
      });
    });
  });

  it("keeps ACP backend management separate from Composer Runtime Profiles", async () => {
    gatewayMock.agentRecords = [
      agentRecord("opencode", ["subagent"], "opencode"),
      agentRecord("cursor", ["peer", "subagent"], "cursor"),
      agentRecord("translate", ["subagent"])
    ];
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      },
      {
        id: "cursor",
        kind: "acp",
        enabled: true,
        label: "Cursor",
        description: null,
        command: "cursor-agent",
        args: ["--acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    const popover = await openAgentRuntimePopover();
    const targets = within(popover).getByRole("radiogroup", { name: "Agent target" });
    expect(within(targets).getByRole("radio", { name: "Psychevo · Psychevo (Native)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "translate · Psychevo (Native)" })).toBeTruthy();
    expect(within(targets).queryByRole("radio", { name: /cursor/i })).toBeNull();
    expect(within(targets).queryByRole("radio", { name: /opencode/i })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/context/read")).toBe(true);
    expect(gatewayMock.requestLog.some((entry) => entry.method === "backend/list")).toBe(true);
  });

  it("does not derive Composer Runtime Profiles from ACP peer entrypoints", async () => {
    gatewayMock.agentRecords = [agentRecord("opencode", ["peer", "subagent"], "opencode")];
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    await selectRuntime("opencode");
    const popover = await openRuntimeProfilePopover();
    const targets = within(popover).getByRole("radiogroup", { name: "Agent target" });
    await waitFor(() => expect(within(targets).getByRole("radio", { name: "opencode · OpenCode (ACP)" }).getAttribute("aria-checked")).toBe("true"));
    expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("opencode");
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/draft/prepare",
      params: expect.objectContaining({ targetId: "target:opencode:opencode" })
    });
    const mode = screen.getByRole("combobox", { name: "Session Mode" }) as HTMLSelectElement;
    expect(mode.options[mode.selectedIndex]?.textContent).toBe("build");
    expect(Array.from(mode.options).map((option) => option.textContent)).toEqual(["build", "plan"]);
    const modelPicker = await openComposerModelPicker();
    expect(within(modelPicker).getByRole("radiogroup", { name: "Model" }).querySelectorAll('[role="radio"]')).toHaveLength(2);
    fireEvent.keyDown(modelPicker, { key: "Escape" });

    gatewayMock.agentRecords = [agentRecord("opencode", ["subagent"], "opencode")];
    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const capabilitiesRegion = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(capabilitiesRegion).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(capabilitiesRegion).findByRole("tab", { name: "ACP Backends" }));
    fireEvent.click(await within(capabilitiesRegion).findByLabelText("opencode peer entrypoint"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          entrypoints: ["subagent"]
        })
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    const nextPopover = await openRuntimeProfilePopover();
    const nextTargets = within(nextPopover).getByRole("radiogroup", { name: "Agent target" });
    expect(within(nextTargets).queryByRole("radio", { name: /OpenCode \(ACP\)/ })).toBeNull();
  });

  it("creates a Profile ACP backend from Capabilities Agents", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const capabilitiesRegion = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(capabilitiesRegion).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(capabilitiesRegion).findByRole("tab", { name: "ACP Backends" }));

    const agentsPanel = await within(capabilitiesRegion).findByRole("region", { name: "Agents" });
    const addButton = within(agentsPanel).getByRole("button", { name: "Add ACP backend" });
    expect(addButton.textContent).toContain("Add backend");
    fireEvent.click(addButton);
    const form = await within(agentsPanel).findByRole("form", { name: "Profile ACP backend" });
    expect(within(form).queryByLabelText("Target")).toBeNull();
    expect((within(form).getByLabelText("ID") as HTMLInputElement).value).toBe("");
    const commandJson = within(form).getByLabelText("Command JSON") as HTMLTextAreaElement;
    expect(JSON.parse(commandJson.value)).toEqual({
      command: "opencode",
      args: ["acp"],
      env: {}
    });
    expect(commandJson.placeholder).toBe("");
    expect(within(form).queryByLabelText("Command")).toBeNull();
    expect(within(form).queryByLabelText("Args")).toBeNull();
    expect(within(form).queryByLabelText("Env")).toBeNull();
    const cwd = within(form).getByLabelText("Backend workspace") as HTMLInputElement;
    expect(cwd.value).toBe("");
    expect(cwd.placeholder).toBe("Defaults to workspace");
    expect(within(form).getByLabelText("Label").closest("label")?.textContent).toContain("Optional");
    expect(within(form).getByLabelText("Description").closest("label")?.textContent).toContain("Optional");
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    expect(within(form).queryByLabelText("Enabled")).toBeNull();
    expect(within(form).queryByText("Entrypoints")).toBeNull();
    fireEvent.change(cwd, { target: { value: "agents" } });
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    fireEvent.change(cwd, { target: { value: "/opt/acp" } });
    expect(within(form).queryByText(/Resolves to/)).toBeNull();
    fireEvent.change(cwd, { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("ID"), { target: { value: "local-acp" } });
    expect((within(form).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.change(commandJson, { target: { value: "{" } });
    expect(within(form).getByText("Command JSON must be valid JSON.")).toBeTruthy();
    expect((within(form).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(commandJson, {
      target: {
        value: JSON.stringify({
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" }
        }, null, 2)
      }
    });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "local-acp",
          target: "profile",
          label: null,
          description: null,
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" },
          cwd: "invocation",
          entrypoints: ["peer", "subagent"],
          clientCapabilities: ["fs.read", "fs.write", "terminal"]
        })
      });
    });
  });

  it("distinguishes an ACP backend from a same-label Runtime Profile without changing its edit value", async () => {
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const capabilitiesRegion = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(capabilitiesRegion).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(capabilitiesRegion).findByRole("tab", { name: "Runtime Profiles" }));
    const runtimeProfile = await within(capabilitiesRegion).findByRole("button", { name: "Runtime Profile opencode" });
    expect(within(runtimeProfile).getByText("OpenCode (ACP)")).toBeTruthy();

    fireEvent.click(within(capabilitiesRegion).getByRole("tab", { name: "ACP Backends" }));
    const agentsPanel = await within(capabilitiesRegion).findByRole("region", { name: "Agents" });
    expect(within(agentsPanel).getByText("OpenCode (ACP)")).toBeTruthy();
    fireEvent.click(within(agentsPanel).getByRole("button", { name: "Edit opencode" }));
    const form = await within(agentsPanel).findByRole("form", { name: "Profile ACP backend" });
    expect((within(form).getByLabelText("Label") as HTMLInputElement).value).toBe("OpenCode");
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          label: "OpenCode"
        })
      });
    });
  });

  it("updates Profile ACP backend enabled and entrypoints from Capabilities Agents", async () => {
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Capabilities" }));
    const capabilitiesRegion = await screen.findByRole("region", { name: "Capabilities" });
    fireEvent.click(within(capabilitiesRegion).getByRole("tab", { name: "Agents" }));
    fireEvent.click(await within(capabilitiesRegion).findByRole("tab", { name: "ACP Backends" }));
    const agentsPanel = await within(capabilitiesRegion).findByRole("region", { name: "Agents" });
    expect(within(agentsPanel).queryByText("Enabled")).toBeNull();
    expect(within(agentsPanel).queryByText("Disabled")).toBeNull();

    fireEvent.click(await within(agentsPanel).findByRole("switch", { name: "Disable opencode" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["peer", "subagent"]
        })
      });
    });

    fireEvent.click(await within(agentsPanel).findByLabelText("opencode peer entrypoint"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["subagent"]
        })
      });
    });
  });
});

function modelProviderHeadings(root: Element): string[] {
  return Array.from(root.querySelectorAll(".modelReasoningProviderHeading"))
    .map((heading) => heading.textContent?.trim() ?? "")
    .filter(Boolean);
}

async function openComposerModelPicker(): Promise<HTMLElement> {
  const existing = screen.queryByRole("dialog", { name: "Model and reasoning" });
  if (existing) return existing;
  fireEvent.click(await screen.findByRole("button", { name: "Model" }));
  return screen.findByRole("dialog", { name: "Model and reasoning" });
}
