import { useEffect, useState } from "react";
import { scopeForCwd, type GatewayClient } from "@psychevo/client";
import {
  AutomationDraftResultSchema,
  AutomationListResultSchema,
  AutomationMutationResultSchema,
  AutomationRunResultSchema,
  type AutomationDraftParams,
  type AutomationDraftView,
  type AutomationWriteParams,
  type GatewayRequestScope
} from "@psychevo/protocol";
import { LIVE_EVENT_REFRESH_SETTLE_MS } from "./app-live-events";
import type {
  MainView,
  WorkbenchAutomation
} from "./types";

type RefreshSnapshot = (
  nextClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

type UseAutomationsParams = {
  activeScope: GatewayRequestScope | null;
  activeWorkbenchCwd: string;
  client: GatewayClient | null;
  initScope: GatewayRequestScope | null;
  mainView: MainView;
  settingsCwd: string | undefined;
  beginExplicitViewSwitch(): number;
  refreshSnapshot: RefreshSnapshot;
  runAction(action: () => Promise<void>): Promise<void>;
  setMobilePanel(value: "history" | "transcript" | "status"): void;
  updateMainView(value: MainView): void;
};

function upsertAutomation(
  current: WorkbenchAutomation[],
  next: WorkbenchAutomation
): WorkbenchAutomation[] {
  const existing = current.some((automation) => automation.id === next.id);
  if (!existing) {
    return [next, ...current];
  }
  return current.map((automation) => automation.id === next.id ? next : automation);
}

export function useAutomations(params: UseAutomationsParams) {
  const [automations, setAutomations] = useState<WorkbenchAutomation[]>([]);
  const [automationsLoading, setAutomationsLoading] = useState(false);
  const [automationsError, setAutomationsError] = useState<string | null>(null);

  function activeAutomationScope(): GatewayRequestScope {
    return params.activeScope ?? params.initScope ?? scopeForCwd(params.settingsCwd ?? window.location.pathname);
  }

  async function refreshAutomations(nextClient: GatewayClient | null = params.client) {
    if (!nextClient) {
      return;
    }
    setAutomationsLoading(true);
    setAutomationsError(null);
    try {
      const result = AutomationListResultSchema.parse(
        await nextClient.request("automation/list", {
          cwd: activeAutomationScope().cwd
        })
      );
      setAutomations(result.automations);
    } catch (error) {
      setAutomationsError(error instanceof Error ? error.message : String(error));
    } finally {
      setAutomationsLoading(false);
    }
  }

  async function saveAutomation(paramsToSave: AutomationWriteParams) {
    if (!params.client) {
      return;
    }
    setAutomationsError(null);
    const result = AutomationMutationResultSchema.parse(
      await params.client.request("automation/write", {
        ...paramsToSave,
        scope: paramsToSave.scope ?? activeAutomationScope()
      })
    );
    setAutomations((current) => upsertAutomation(current, result.automation));
  }

  async function draftAutomation(paramsToDraft: AutomationDraftParams): Promise<AutomationDraftView> {
    if (!params.client) {
      throw new Error("Gateway is not connected.");
    }
    setAutomationsError(null);
    try {
      const result = AutomationDraftResultSchema.parse(
        await params.client.request("automation/draft", {
          ...paramsToDraft,
          scope: paramsToDraft.scope ?? activeAutomationScope()
        })
      );
      return result.draft;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setAutomationsError(message);
      throw error;
    }
  }

  async function runAutomation(id: string) {
    if (!params.client) {
      return;
    }
    setAutomationsError(null);
    const result = AutomationRunResultSchema.parse(
      await params.client.request("automation/run", {
        automationId: id,
        trigger: "manual"
      })
    );
    setAutomations((current) => upsertAutomation(current, result.automation));
    window.setTimeout(() => {
      void refreshAutomations(params.client);
    }, LIVE_EVENT_REFRESH_SETTLE_MS);
  }

  async function pauseAutomation(id: string) {
    await setAutomationEnabled(id, false);
  }

  async function resumeAutomation(id: string) {
    await setAutomationEnabled(id, true);
  }

  async function setAutomationEnabled(id: string, enabled: boolean) {
    if (!params.client) {
      return;
    }
    setAutomationsError(null);
    const result = AutomationMutationResultSchema.parse(
      await params.client.request(enabled ? "automation/resume" : "automation/pause", {
        automationId: id
      })
    );
    setAutomations((current) => upsertAutomation(current, result.automation));
  }

  async function deleteAutomation(id: string) {
    if (!params.client) {
      return;
    }
    setAutomationsError(null);
    await params.client.request("automation/delete", { automationId: id });
    setAutomations((current) => current.filter((automation) => automation.id !== id));
  }

  function openAutomationThread(threadId: string) {
    void params.runAction(async () => {
      const epoch = params.beginExplicitViewSwitch();
      await params.refreshSnapshot(params.client, threadId, undefined, false, epoch);
      params.updateMainView("transcript");
      params.setMobilePanel("transcript");
    });
  }

  useEffect(() => {
    if (!params.client || params.mainView !== "automations") {
      return;
    }
    void refreshAutomations(params.client);
  }, [params.client, params.mainView, params.activeWorkbenchCwd]);

  return {
    automations,
    automationsError,
    automationsLoading,
    deleteAutomation,
    draftAutomation,
    openAutomationThread,
    pauseAutomation,
    refreshAutomations,
    resumeAutomation,
    runAutomation,
    saveAutomation
  };
}
