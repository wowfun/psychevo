import type { ChannelUpdateParams, SettingsReadResult } from "@psychevo/protocol";

export type ChannelUpdateDraft = Partial<Omit<ChannelUpdateParams, "id" | "scope">>;
export type ChannelSettingsControls = SettingsReadResult["controls"];
