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
    expect(within(settingsRegion).getByRole("button", { name: "Agents" })).toBeTruthy();
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
    fireEvent.click(within(zenRow).getByRole("button", { name: "Fetch" }));
    expect(await within(modelsPanel).findByText("OpenCode Zen: catalog updated")).toBeTruthy();
    expect(within(zenRow).getByRole("button", { name: "2 models" })).toBeTruthy();
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
    const modelButton = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(modelButton.textContent).toContain("mimo-v2.5-free Default");
    });
    fireEvent.click(modelButton);
    const modelPopover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    const freeComposerRow = within(modelPopover).getByRole("radio", { name: /mimo-v2\.5-free/ });
    expect(freeComposerRow.getAttribute("data-model-value")).toBe("opencode-zen/mimo-v2.5-free");
    expect(freeComposerRow.getAttribute("data-model-free")).toBe("true");
    expect(freeComposerRow.querySelector(".modelReasoningFreeBadge")?.textContent).toBe("Free");
    expect(modelProviderHeadings(modelPopover)).toContain("OpenCode Zen");
    expect(freeComposerRow.getAttribute("title")).toBeNull();
    fireEvent.keyDown(modelPopover, { key: "Escape" });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "use the saved default" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "use the saved default" }],
          model: "opencode-zen/mimo-v2.5-free",
          reasoningEffort: null
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

    const initialModelButton = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(initialModelButton.textContent).toContain("xiaomi-token-high Default");
    });

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Models" }));
    const modelsPanel = await within(settingsRegion).findByRole("region", { name: "Models" });
    const zenRow = within(modelsPanel).getByText("OpenCode Zen").closest(".modelProviderRow") as HTMLElement;
    fireEvent.click(within(zenRow).getByRole("button", { name: "Fetch" }));
    expect(await within(modelsPanel).findByText("OpenCode Zen: catalog updated")).toBeTruthy();

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
    const modelButton = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(modelButton.textContent).toContain("xiaomi-token-high Default");
    });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "keep scoped model" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "keep scoped model" }],
          model: "xiaomi/xiaomi-token-high"
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
        { provider: "opencode-zen", id: "big-pickle", value: "opencode-zen/big-pickle", label: null, providerLabel: "OpenCode Zen", free: true, contextLimit: null, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
      ]
    };

    render(<App />);

    const composerModelButton = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(composerModelButton.textContent).toContain("xiaomi-token-high Default");
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
    const currentComposerModelButton = await screen.findByRole("button", { name: "Model" });
    await waitFor(() => {
      expect(currentComposerModelButton.textContent).toContain("xiaomi-token-high Default");
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

  it("switches Settings sections while keeping session controls in the composer", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    expect(within(settingsRegion).getByRole("button", { name: "Agents" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("region", { name: "Agents" })).toBeTruthy();
    expect(within(settingsRegion).getByText("Profile ACP Backends")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Translate user messages")).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Agent" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Model" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Permission mode" })).toBeNull();

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    expect(await screen.findByRole("button", { name: "Agent" })).toBeTruthy();
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
    expect(within(detailPage).getByRole("option", { name: "custom/current-model (current)" })).toBeTruthy();
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
    fireEvent.click(within(detailPage).getByRole("button", { name: "Bypass permissions" }));
    fireEvent.click(within(detailPage).getByText("Advanced diagnostics"));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "channel/source/list")).toBe(true);
    });
    expect(within(detailPage).getByText("Remote lanes")).toBeTruthy();
    expect(within(detailPage).getByText("Channel lane")).toBeTruthy();
    expect(within(detailPage).getByText("/tmp/project")).toBeTruthy();
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
      workdir: "/tmp/channel-workspace",
      model: "openai/gpt-4o",
      permissionMode: "bypassPermissions",
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

  it("keeps internal WeChat env names out of the default detail save surface", async () => {
    gatewayMock.channelRecords = [
      {
        id: "wechat",
        channel: "wechat",
        domain: "wechat",
        enabled: true,
        label: "WeChat Ops",
        transport: "polling",
        workdir: null,
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
        workdir: "/tmp/project",
        project: { workdir: "/tmp/project", label: "project", displayPath: "/tmp/project" },
        sessions: [],
        hiddenCount: 0,
        nextCursor: null
      },
      {
        workdir: "/tmp/recent-ops",
        project: { workdir: "/tmp/recent-ops", label: "recent-ops", displayPath: "/tmp/recent-ops" },
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
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(1);
    });
    expect(within(detailPage).getByText("Next message will start in the new workspace.")).toBeTruthy();
    expect(gatewayMock.requestLog.find((entry) => entry.method === "channel/update")?.params).toEqual(expect.objectContaining({
      id: "release",
      workdir: "/tmp/recent-ops"
    }));

    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    });
    fireEvent.change(workspacePreset, { target: { value: "" } });
    expect(workspaceInput.value).toBe("");
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(2);
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").at(-1)?.params).toEqual(expect.objectContaining({
      id: "release",
      workdir: ""
    }));

    await waitFor(() => {
      expect((within(detailPage).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    });
    fireEvent.change(workspaceInput, { target: { value: "/tmp/manual-channel" } });
    expect(workspacePreset.value).toBe("__manual__");
    expect(within(detailPage).getByRole("option", { name: "Manual path" })).toBeTruthy();
    fireEvent.click(within(detailPage).getAllByRole("button", { name: "Save" })[0]!);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").length).toBe(3);
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "channel/update").at(-1)?.params).toEqual(expect.objectContaining({
      id: "release",
      workdir: "/tmp/manual-channel"
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
        workdir: null,
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
        workdir: null,
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

  it("renders model and reasoning in one grouped selector", async () => {
    render(<App />);

    const modelButton = await screen.findByRole("button", { name: "Model" });
    expect(modelButton.textContent).toContain("xiaomi-token-high Default");
    expect(modelButton.textContent).not.toContain("xiaomi/xiaomi-token-high");
    expect(modelButton.getAttribute("title")).toBe("xiaomi/xiaomi-token-high / Default");
    expect(screen.queryByRole("combobox", { name: "Model" })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Variant" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Select model" })).toBeNull();

    fireEvent.click(modelButton);
    const popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    const filter = within(popover).getByRole("searchbox", { name: "Model filter" });
    expect(filter).toBeTruthy();
    const modelGroup = within(popover).getByRole("radiogroup", { name: "Model" });
    expect(within(modelGroup).getByRole("radio", { name: /xiaomi-token-high/ }).getAttribute("aria-checked")).toBe("true");
    expect(within(modelGroup).getByRole("radio", { name: /xiaomi-token-high/ }).getAttribute("data-model-value")).toBe("xiaomi/xiaomi-token-high");
    expect(within(modelGroup).getByRole("radio", { name: /xiaomi-token-high/ }).getAttribute("title")).toBeNull();
    expect(within(modelGroup).queryByText(/xiaomi\/xiaomi-token-high/)).toBeNull();
    expect((popover as HTMLElement).style.getPropertyValue("--pevo-model-picker-popover-width")).toBe("25ch");
    expect(modelProviderHeadings(modelGroup)).toEqual(["Xiaomi", "OpenAI", "Xiaomi"]);
    expect(within(modelGroup).getByRole("radio", { name: /xiaomi-token-high/ }).querySelector("small")).toBeNull();
    expect(within(modelGroup).getByRole("radio", { name: /xiaomi-token-high/ }).querySelector(".modelReasoningFreeBadge")).toBeNull();
    fireEvent.change(filter, { target: { value: "gpt" } });
    expect(within(modelGroup).queryByRole("radio", { name: /xiaomi-token-high/ })).toBeNull();
    expect(within(modelGroup).getByRole("radio", { name: /gpt-4o/ })).toBeTruthy();
    expect((popover as HTMLElement).style.getPropertyValue("--pevo-model-picker-popover-width")).toBe("24ch");
    expect(modelProviderHeadings(modelGroup)).toEqual(["OpenAI"]);
    fireEvent.change(filter, { target: { value: "" } });
    const reasoningGroup = within(popover).getByRole("radiogroup", { name: "Reasoning" });
    expect(within(reasoningGroup).getByRole("radio", { name: /Default/ }).getAttribute("aria-checked")).toBe("true");
    expect(within(reasoningGroup).getByRole("radio", { name: "High" })).toBeTruthy();
    expect(within(reasoningGroup).queryByText("Explicit reasoning effort")).toBeNull();
    expect(screen.queryByText("medium")).toBeNull();
  });

  it("groups adjacent visible model providers after recent ordering", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Model" }));
    let popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(popover).getByRole("radio", { name: /gpt-4o/ }));
    fireEvent.click(within(popover).getByRole("radio", { name: /xiaomi-token-high/ }));

    popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    const modelGroup = within(popover).getByRole("radiogroup", { name: "Model" });
    expect(modelProviderHeadings(modelGroup)).toEqual(["Xiaomi", "OpenAI", "Xiaomi"]);
    expect(Array.from(modelGroup.querySelectorAll(".modelReasoningRow")).map((row) => row.getAttribute("data-model-value"))).toEqual([
      "xiaomi/xiaomi-token-high",
      "openai/gpt-4o",
      "xiaomi/xiaomi-token-low"
    ]);
  });

  it("persists composer model choices to shared model state", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Model" }));
    let popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(popover).getByRole("radio", { name: /gpt-4o/ }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "model/state/set",
        params: expect.objectContaining({
          workdir: "/tmp/project",
          model: "openai/gpt-4o",
          reasoningEffort: null
        })
      });
    });

    fireEvent.click(within(popover).getByRole("radio", { name: /xiaomi-token-low/ }));
    popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(popover).getByRole("radio", { name: "Medium" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "model/state/set",
        params: expect.objectContaining({
          workdir: "/tmp/project",
          model: "xiaomi/xiaomi-token-low",
          reasoningEffort: "medium"
        })
      });
    });
  });

  it("adapts reasoning options when switching models", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Model" }));
    let popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(popover).getByRole("radio", { name: /High/ }));
    expect((within(popover).getByRole("radio", { name: /High/ })).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(within(popover).getByRole("radio", { name: /gpt-4o/ }));

    popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    const reasoningGroup = within(popover).getByRole("radiogroup", { name: "Reasoning" });
    expect(within(reasoningGroup).getByRole("radio", { name: /Default/ }).getAttribute("aria-checked")).toBe("true");
    expect(within(reasoningGroup).queryByRole("radio", { name: /High/ })).toBeNull();
  });

  it("blocks prompt turns until a concrete provider-qualified model is selected", async () => {
    gatewayMock.model = null;
    gatewayMock.modelStatus = "unconfigured";

    render(<App />);

    expect((await screen.findAllByText("Select model")).length).toBeGreaterThan(0);
    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello" } });
    const sendButton = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    expect(sendButton.disabled).toBe(true);
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    const popover = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(popover).getByRole("radio", { name: /gpt-4o/ }));
    expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "hello" }],
          model: "openai/gpt-4o"
        })
      });
    });
  });

  it("keeps ACP peer backends in Runtime instead of the composer Agent selector", async () => {
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
    const agentGroup = within(popover).getByRole("radiogroup", { name: "Main agent" });
    expect(within(agentGroup).getByRole("radio", { name: "Default Agent" })).toBeTruthy();
    expect(within(agentGroup).queryByRole("radio", { name: "cursor" })).toBeNull();
    expect(within(agentGroup).queryByRole("radio", { name: "opencode" })).toBeNull();

    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    expect(within(runtimeGroup).getByRole("radio", { name: "Native Runtime" })).toBeTruthy();
    expect(within(runtimeGroup).getByRole("radio", { name: "Cursor" })).toBeTruthy();
    expect(within(runtimeGroup).queryByRole("radio", { name: "OpenCode" })).toBeNull();
  });

  it("clears a selected ACP runtime when its peer entrypoint is disabled", async () => {
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
    const popover = await openAgentRuntimePopover();
    const runtimeGroup = within(popover).getByRole("radiogroup", { name: "Runtime" });
    await waitFor(() => expect(within(runtimeGroup).getByRole("radio", { name: "OpenCode" }).getAttribute("aria-checked")).toBe("true"));

    gatewayMock.agentRecords = [agentRecord("opencode", ["subagent"], "opencode")];
    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));
    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });
    fireEvent.click(await within(agentsPanel).findByLabelText("opencode peer entrypoint"));

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

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    const nextPopover = await openAgentRuntimePopover();
    const nextRuntimeGroup = within(nextPopover).getByRole("radiogroup", { name: "Runtime" });
    await waitFor(() => expect(within(nextRuntimeGroup).getByRole("radio", { name: "Native Runtime" }).getAttribute("aria-checked")).toBe("true"));
    expect(within(nextRuntimeGroup).queryByRole("radio", { name: "OpenCode" })).toBeNull();
  });

  it("creates a Profile ACP backend from the generic Settings Agents add action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });
    const addButton = within(agentsPanel).getByRole("button", { name: "Add ACP backend" });
    expect(addButton.textContent).toBe("");
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
    const cwd = within(form).getByLabelText("CWD") as HTMLInputElement;
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

  it("updates Profile ACP backend enabled and entrypoints from Settings Agents rows", async () => {
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

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));
    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });

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
