import {
  createContext,
  useContext,
  useMemo,
  type ReactNode
} from "react";
import type { GatewayClient } from "@psychevo/client";
import type { WorkspaceFileWriteResult } from "@psychevo/protocol";

export type WorkspaceFileGatewayAdapter = {
  client: GatewayClient | null;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onOpenHtmlPreview?: ((path: string, content: string) => void) | undefined;
  onSave?: ((
    path: string,
    content: string,
    expectedRevision: string | null,
    force: boolean
  ) => Promise<WorkspaceFileWriteResult>) | undefined;
};

const WorkspaceFileGatewayAdapterContext = createContext<WorkspaceFileGatewayAdapter>({
  client: null
});

export function WorkspaceFileGatewayAdapterProvider({
  children,
  client,
  onCopyText,
  onOpenHtmlPreview,
  onSave
}: WorkspaceFileGatewayAdapter & { children: ReactNode }) {
  const value = useMemo(() => ({
    client,
    onCopyText,
    onOpenHtmlPreview,
    onSave
  }), [client, onCopyText, onOpenHtmlPreview, onSave]);
  return (
    <WorkspaceFileGatewayAdapterContext.Provider value={value}>
      {children}
    </WorkspaceFileGatewayAdapterContext.Provider>
  );
}

export function useWorkspaceFileGatewayAdapter(): WorkspaceFileGatewayAdapter {
  return useContext(WorkspaceFileGatewayAdapterContext);
}
