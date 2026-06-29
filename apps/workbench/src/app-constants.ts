import { scopeForCwd } from "@psychevo/client";
import type { ThreadSnapshot } from "@psychevo/protocol";
import { idleActivity } from "./session-utils";

export const EMPTY_SNAPSHOT: ThreadSnapshot = {
  source: { kind: "web", rawId: "pending", lifetime: "persistent", rawIdentity: null, visibleName: null },
  scope: scopeForCwd(""),
  thread: null,
  entries: [],
  activity: idleActivity(),
  pendingPermissions: [],
  pendingClarifies: []
};
