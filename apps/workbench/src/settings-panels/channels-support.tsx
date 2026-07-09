import { useEffect, useState } from "react";
import { Activity, MessageCircle, PlugZap, RotateCcw, Wrench } from "lucide-react";
import type { ChannelWechatQrPollResult, ChannelWechatQrStartResult } from "@psychevo/protocol";
import type { SessionBrowserWorkspaceState, WorkbenchChannel, WorkbenchChannelDoctor } from "../types";
import type { ChannelSettingsControls, ChannelUpdateDraft } from "./types";

export type ChannelChoice = "wechat" | "telegram" | "feishu" | "lark";

export const CHANNEL_CHOICES: ChannelChoice[] = ["wechat", "telegram", "feishu", "lark"];


export type ChannelSettingsDraft = {
  label: string;
  enabled: boolean;
  cwd: string;
  runtimeRef: string;
  model: string;
  permissionMode: string;
  requireMention: boolean;
  allowUsersText: string;
  allowGroupsText: string;
  credentialEnv: string;
};

export function sectionDomId(title: string): string {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

const DEFAULT_PERMISSION_MODE_OPTIONS = ["default", "acceptEdits", "dontAsk", "bypassPermissions"];
export const CHANNEL_WORKSPACE_MANUAL_VALUE = "__manual__";

export function channelWorkspaceOptions(workspaces: SessionBrowserWorkspaceState[]): string[] {
  const seen = new Set<string>();
  const options: string[] = [];
  for (const workspace of workspaces) {
    const cwd = workspace.cwd.trim();
    if (!cwd || seen.has(cwd)) {
      continue;
    }
    seen.add(cwd);
    options.push(cwd);
  }
  return options;
}

export function channelWorkspaceSelectValue(value: string, options: string[]): string {
  const cwd = value.trim();
  if (!cwd) {
    return "";
  }
  return options.includes(cwd) ? cwd : CHANNEL_WORKSPACE_MANUAL_VALUE;
}

export function channelWorkspaceOptionLabel(cwd: string): string {
  const normalized = cwd.trim();
  const trimmed = normalized.replace(/[\\/]+$/, "");
  const segments = trimmed.split(/[\\/]/).filter(Boolean);
  const basename = segments[segments.length - 1] ?? "Workspace";
  return basename && basename !== normalized ? `${basename} - ${normalized}` : normalized;
}

export function channelDraftFromChannel(channel: WorkbenchChannel): ChannelSettingsDraft {
  return {
    label: channel.label ?? "",
    enabled: channel.enabled,
    cwd: channel.cwd ?? "",
    runtimeRef: channel.runtimeRef ?? "",
    model: channel.model ?? "",
    permissionMode: channel.permissionMode ?? "default",
    requireMention: channel.requireMention,
    allowUsersText: channel.allowlist.users.join("\n"),
    allowGroupsText: channel.allowlist.groups.join("\n"),
    credentialEnv: channel.credential.env ?? ""
  };
}

export function channelUpdateDraftFromDraft(draft: ChannelSettingsDraft): ChannelUpdateDraft {
  return {
    label: draft.label.trim(),
    enabled: draft.enabled,
    cwd: draft.cwd.trim(),
    runtimeRef: draft.runtimeRef.trim(),
    model: draft.model.trim(),
    permissionMode: draft.permissionMode,
    requireMention: draft.requireMention,
    allowUsers: splitChannelListText(draft.allowUsersText),
    allowGroups: splitChannelListText(draft.allowGroupsText),
    credentialEnv: draft.credentialEnv.trim()
  };
}

export function channelDraftSignature(draft: ChannelSettingsDraft): string {
  return JSON.stringify(channelUpdateDraftFromDraft(draft));
}

export function splitChannelListText(value: string): string[] {
  const seen = new Set<string>();
  const items: string[] = [];
  for (const part of value.split(/[,\n]/)) {
    const item = part.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    items.push(item);
  }
  return items;
}

export function channelPermissionOptions(
  controls: ChannelSettingsControls,
  channel: WorkbenchChannel,
  draft: ChannelSettingsDraft
): string[] {
  return uniqueStrings([
    ...(controls?.permissionModeOptions ?? DEFAULT_PERMISSION_MODE_OPTIONS),
    "default",
    channel.permissionMode ?? "",
    draft.permissionMode
  ]).filter((value) => DEFAULT_PERMISSION_MODE_OPTIONS.includes(value));
}

export function channelModelOptions(
  controls: ChannelSettingsControls,
  channel: WorkbenchChannel,
  draft: ChannelSettingsDraft
): string[] {
  return uniqueStrings([
    ...(controls?.modelOptions ?? []),
    channel.model ?? "",
    draft.model
  ]).filter(Boolean);
}

export function channelRuntimeProfileOptions(
  channel: WorkbenchChannel,
  draft: ChannelSettingsDraft
): string[] {
  return uniqueStrings([
    "",
    "native",
    "codex",
    "opencode",
    channel.runtimeRef ?? "",
    draft.runtimeRef
  ]);
}

export function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const item = value.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    out.push(item);
  }
  return out;
}

export function permissionModeLabel(value: string): string {
  switch (value) {
    case "acceptEdits":
      return "Accept edits";
    case "dontAsk":
      return "Inline approvals";
    case "bypassPermissions":
      return "Bypass permissions";
    default:
      return "Profile default";
  }
}

export function modelOptionLabel(value: string, channel: WorkbenchChannel, controls: ChannelSettingsControls): string {
  if (channel.model === value && !(controls?.modelOptions ?? []).includes(value)) {
    return `${value} (current)`;
  }
  return value;
}

export function runtimeProfileOptionLabel(value: string): string {
  switch (value) {
    case "":
      return "Profile default";
    case "native":
      return "Native";
    case "codex":
      return "Codex";
    case "opencode":
      return "OpenCode";
    default:
      return value;
  }
}

type WechatQrSetupState = {
  done: boolean;
  error: string | null;
  expiresAtMs: number | null;
  intervalMs: number;
  loading: boolean;
  message: string;
  qrImage: string | null;
  qrSvg: string | null;
  qrUrl: string | null;
  sessionId: string | null;
  status: string;
};

const EMPTY_WECHAT_QR_SETUP: WechatQrSetupState = {
  done: false,
  error: null,
  expiresAtMs: null,
  intervalMs: 3000,
  loading: false,
  message: "Generate a QR code, scan it with WeChat, then Psychevo saves the iLink token locally.",
  qrImage: null,
  qrSvg: null,
  qrUrl: null,
  sessionId: null,
  status: "idle"
};

export function ChannelSetupCard({
  channel,
  disabled,
  existingChannel,
  onPollWechatQrSetup,
  onStartWechatQrSetup
}: {
  channel: ChannelChoice;
  disabled: boolean;
  existingChannel: WorkbenchChannel | null;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
}) {
  const [wechatQr, setWechatQr] = useState<WechatQrSetupState>(EMPTY_WECHAT_QR_SETUP);
  const [qrNowMs, setQrNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || wechatQr.done || disabled) {
      return undefined;
    }
    const sessionId = wechatQr.sessionId;
    const timer = window.setInterval(() => {
      void onPollWechatQrSetup(sessionId)
        .then((result) => {
          const terminal = isWechatQrTerminalStatus(result.status, result.done);
          setWechatQr((current) => ({
            ...current,
            done: result.done,
            error: terminal && !result.done ? result.message : null,
            expiresAtMs: terminal ? null : (result.expiresAtMs ?? current.expiresAtMs),
            loading: false,
            message: result.message,
            qrImage: terminal ? null : current.qrImage,
            qrSvg: terminal ? null : current.qrSvg,
            qrUrl: terminal ? null : current.qrUrl,
            sessionId: terminal ? null : current.sessionId,
            status: result.status
          }));
        })
        .catch((error: unknown) => {
          const terminal = isWechatQrSessionLostError(error);
          setWechatQr((current) => ({
            ...current,
            error: qrSetupErrorMessage(error),
            expiresAtMs: terminal ? null : current.expiresAtMs,
            loading: false,
            qrImage: terminal ? null : current.qrImage,
            qrSvg: terminal ? null : current.qrSvg,
            qrUrl: terminal ? null : current.qrUrl,
            sessionId: terminal ? null : current.sessionId,
            status: "error"
          }));
        });
    }, wechatQr.intervalMs);
    return () => window.clearInterval(timer);
  }, [channel, disabled, onPollWechatQrSetup, wechatQr.done, wechatQr.intervalMs, wechatQr.sessionId]);

  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || !wechatQr.expiresAtMs || wechatQr.done || wechatQr.status === "expired") {
      return undefined;
    }
    setQrNowMs(Date.now());
    const timer = window.setInterval(() => setQrNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [channel, wechatQr.done, wechatQr.expiresAtMs, wechatQr.sessionId, wechatQr.status]);

  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || !wechatQr.expiresAtMs || wechatQr.done || wechatQr.status === "expired") {
      return;
    }
    if (qrNowMs < wechatQr.expiresAtMs) {
      return;
    }
    setWechatQr((current) => {
      if (!current.sessionId || !current.expiresAtMs || current.done || current.status === "expired" || qrNowMs < current.expiresAtMs) {
        return current;
      }
      return {
        ...current,
        error: "WeChat QR session expired. Generate a new code.",
        expiresAtMs: null,
        loading: false,
        message: "WeChat QR session expired. Generate a new code.",
        qrImage: null,
        qrSvg: null,
        qrUrl: null,
        sessionId: null,
        status: "expired"
      };
    });
  }, [channel, qrNowMs, wechatQr.done, wechatQr.expiresAtMs, wechatQr.sessionId, wechatQr.status]);

  async function startWechatQr() {
    setQrNowMs(Date.now());
    setWechatQr((current) => ({ ...current, error: null, loading: true, status: "starting" }));
    try {
      const result = await onStartWechatQrSetup();
      setWechatQr({
        done: false,
        error: null,
        expiresAtMs: result.expiresAtMs,
        intervalMs: result.intervalMs,
        loading: false,
        message: result.message,
        qrImage: result.qrImage ?? null,
        qrSvg: result.qrSvg,
        qrUrl: result.qrUrl,
        sessionId: result.sessionId,
        status: result.status
      });
    } catch (error) {
      setWechatQr((current) => ({
        ...current,
        error: qrSetupErrorMessage(error),
        loading: false,
        status: "error"
      }));
    }
  }

  async function checkWechatQr() {
    if (!wechatQr.sessionId) {
      return;
    }
    setWechatQr((current) => ({ ...current, error: null, loading: true }));
    try {
      const result = await onPollWechatQrSetup(wechatQr.sessionId);
      const terminal = isWechatQrTerminalStatus(result.status, result.done);
      setWechatQr((current) => ({
        ...current,
        done: result.done,
        error: terminal && !result.done ? result.message : null,
        expiresAtMs: terminal ? null : (result.expiresAtMs ?? current.expiresAtMs),
        loading: false,
        message: result.message,
        qrImage: terminal ? null : current.qrImage,
        qrSvg: terminal ? null : current.qrSvg,
        qrUrl: terminal ? null : current.qrUrl,
        sessionId: terminal ? null : current.sessionId,
        status: result.status
      }));
    } catch (error) {
      const terminal = isWechatQrSessionLostError(error);
      setWechatQr((current) => ({
        ...current,
        error: qrSetupErrorMessage(error),
        expiresAtMs: terminal ? null : current.expiresAtMs,
        loading: false,
        qrImage: terminal ? null : current.qrImage,
        qrSvg: terminal ? null : current.qrSvg,
        qrUrl: terminal ? null : current.qrUrl,
        sessionId: terminal ? null : current.sessionId,
        status: "error"
      }));
    }
  }

  if (channel === "wechat") {
    const reconnectRequired = existingChannel?.runner.reason === "needs_qr_login";
    const loginPending = existingChannel?.runner.reason === "qr_login_pending";
    const connectedWechat = existingChannel && existingChannel.credential.status === "present" && existingChannel.allowlist.status === "present" && !reconnectRequired && !loginPending;
    if (loginPending && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardPending">
          <div className="channelConnectedMark" aria-hidden>
            <Activity size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat polling is starting</strong>
            <span>{wechatQr.done && wechatQr.message ? wechatQr.message : "Credentials are saved. Gateway is starting polling."}</span>
            <small>Send a DM to the iLink bot while the Gateway starts polling.</small>
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>qr_login_pending</span>
            </div>
          </div>
        </div>
      );
    }
    if (reconnectRequired && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardReconnect">
          <div className="channelConnectedMark" aria-hidden>
            <RotateCcw size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat reconnect required</strong>
            <span>The iLink login expired. Generate a new QR code and scan it again to resume polling.</span>
            {existingChannel?.runner.lastError && <small className="agentSurfaceWarning">{existingChannel.runner.lastError}</small>}
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>needs_qr_login</span>
            </div>
          </div>
        </div>
      );
    }
    if (connectedWechat && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardConnected">
          <div className="channelConnectedMark" aria-hidden>
            <PlugZap size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat connected</strong>
            <span>Credential and DM allowlist are present. The Gateway runner state is {existingChannel.runner.state}.</span>
            {existingChannel.runner.lastError && <small className="agentSurfaceWarning">{existingChannel.runner.lastError}</small>}
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>DM allowlist</span>
            </div>
          </div>
        </div>
      );
    }
    return (
      <div className="channelSetupCard channelSetupCardWechat">
        <div className="channelWechatQrBox" aria-label="WeChat QR code">
          {wechatQr.qrImage ? (
            <img alt="WeChat QR code" className="channelWechatQrImage" src={wechatQr.qrImage} />
          ) : wechatQr.qrSvg ? (
            <div className="channelWechatQrSvg" dangerouslySetInnerHTML={{ __html: wechatQr.qrSvg }} />
          ) : (
            <QrPlaceholder />
          )}
        </div>
        <div className="channelWechatSetupBody" aria-live="polite">
          <strong>WeChat setup</strong>
          <span>{wechatQr.message}</span>
          {wechatQr.done && <small>The token is saved in the active profile .env.</small>}
          {wechatQr.expiresAtMs && !wechatQr.done && (
            <small className="channelWechatTimer">{formatQrTimeLeft(wechatQr.expiresAtMs, qrNowMs)}</small>
          )}
          {wechatQr.error && <small className="agentSurfaceWarning">{wechatQr.error}</small>}
          <div className="channelWechatActions">
            <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
              <MessageCircle size={13} />
              <span>{wechatQr.status === "error" || wechatQr.status === "expired" || wechatQr.status === "needs_qr_login" ? "Generate again" : "Generate QR"}</span>
            </button>
            <button disabled={disabled || wechatQr.loading || !wechatQr.sessionId || wechatQr.done} onClick={() => void checkWechatQr()} type="button">
              <Wrench size={13} />
              <span>Check status</span>
            </button>
          </div>
          <div className="channelSetupFields">
            <span>WECHAT_BOT_TOKEN</span>
            <span>WECHAT_ACCOUNT_ID</span>
            <span>DM allowlist</span>
          </div>
        </div>
      </div>
    );
  }
  const setup = channelSetupCopy(channel);
  return (
    <div className="channelSetupCard">
      <strong>{setup.title}</strong>
      <span>{setup.primary}</span>
      <code>{setup.command}</code>
      <div className="channelSetupFields">
        {setup.fields.map((field) => (
          <span key={field}>{field}</span>
        ))}
      </div>
    </div>
  );
}

export function QrPlaceholder() {
  return (
    <div className="channelQrPlaceholder" aria-hidden>
      <span />
      <span />
      <span />
      <span />
    </div>
  );
}

export function channelRuntimeDefaultsSummary(draft: ChannelSettingsDraft): string {
  const runtime = runtimeProfileOptionLabel(draft.runtimeRef || "");
  const model = draft.model.trim() || "profile model";
  const workspace = draft.cwd.trim() ? "custom workspace" : "default workspace";
  return `${runtime} · ${permissionModeLabel(draft.permissionMode)} · ${model} · ${workspace}`;
}

export function ChannelHealthItem({
  label,
  tone,
  value
}: {
  label: string;
  tone: "danger" | "muted" | "ok" | "warning";
  value: string;
}) {
  return (
    <span className={`channelHealthItem is-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </span>
  );
}

export function formatQrTimeLeft(expiresAtMs: number, nowMs: number): string {
  const seconds = Math.max(0, Math.ceil((expiresAtMs - nowMs) / 1000));
  if (seconds === 0) {
    return "QR expired";
  }
  return `${seconds}s left`;
}

export function isWechatQrTerminalStatus(status: string, done: boolean): boolean {
  return done || status === "expired" || status === "needs_qr_login";
}

export function isWechatQrSessionLostError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes("QR session not found");
}

export function qrSetupErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("QR session not found")) {
    return "This QR session has expired, completed, or was created before the Gateway restarted. Generate a new code to reconnect.";
  }
  return message;
}

export function channelSetupCopy(channel: ChannelChoice): { command: string; fields: string[]; primary: string; title: string } {
  switch (channel) {
    case "wechat":
      return {
        title: "WeChat setup",
        primary: "Generate a QR code, scan it with WeChat, and store the iLink token in the active profile.",
        command: "pevo gateway setup --channel wechat --qr",
        fields: ["QR login", "WECHAT_BOT_TOKEN", "WECHAT_ACCOUNT_ID", "allow_users"]
      };
    case "telegram":
      return {
        title: "Telegram setup",
        primary: "Create a bot with BotFather and paste the token through stdin.",
        command: "pevo gateway setup --channel telegram --id telegram --allow-user CHAT_ID --credential-stdin",
        fields: ["BotFather token", "TELEGRAM_BOT_TOKEN", "allow_users or allow_groups"]
      };
    case "feishu":
      return {
        title: "Feishu setup",
        primary: "Configure app id and secret env vars for the Feishu long-connection adapter.",
        command: "pevo gateway setup --channel feishu --id feishu --allow-group OPEN_CHAT_ID --credential-stdin",
        fields: ["FEISHU_APP_ID", "FEISHU_APP_SECRET", "allow_groups"]
      };
    case "lark":
      return {
        title: "Lark setup",
        primary: "Configure app id and secret env vars for the Lark long-connection adapter.",
        command: "pevo gateway setup --channel lark --id lark --allow-group OPEN_CHAT_ID --credential-stdin",
        fields: ["LARK_APP_ID", "LARK_APP_SECRET", "allow_groups"]
      };
  }
}

export function ChannelStatusPill({ status }: { status: string }) {
  return <small className={`channelStatusPill is-${status} is-${channelStatusTone(status)}`}>{status}</small>;
}

export function channelDoctorOk(doctor: WorkbenchChannelDoctor): boolean {
  return doctor.checks.every((check) => check.status === "ok" || check.status === "skipped");
}

export function channelRuntimeSummary(channel: WorkbenchChannel, fallbackCwd: string): string {
  const runtime = runtimeProfileOptionLabel(channel.runtimeRef ?? "");
  const model = channel.model ?? "default model";
  const cwd = channel.cwd ?? fallbackCwd;
  return `${runtime} · ${model} · ${cwd}`;
}

export function channelRunnerTone(status: string): "danger" | "muted" | "ok" | "warning" {
  switch (status) {
    case "running":
      return "ok";
    case "blocked":
      return "warning";
    case "error":
      return "danger";
    default:
      return "muted";
  }
}

export function formatRunnerActivity(channel: WorkbenchChannel): string {
  if (channel.runner.reason === "qr_login_pending") {
    return "polling start pending";
  }
  if (channel.runner.reason === "needs_qr_login") {
    return "QR reconnect required";
  }
  if (channel.runner.lastOutboundAtMs) {
    return `outbound ${formatRunnerTimestamp(channel.runner.lastOutboundAtMs)}`;
  }
  if (channel.runner.lastInboundAtMs) {
    return `inbound ${formatRunnerTimestamp(channel.runner.lastInboundAtMs)}`;
  }
  if (channel.runner.lastPollAtMs) {
    return `poll ${formatRunnerTimestamp(channel.runner.lastPollAtMs)}`;
  }
  if (channel.runner.reason) {
    return channel.runner.reason;
  }
  return channel.runner.lastError ?? "no activity yet";
}

export function formatRunnerTimestamp(value: number | null | undefined): string {
  if (!value) {
    return "never";
  }
  return new Date(value).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function channelAllowlistSummary(channel: WorkbenchChannel): string {
  const parts = [];
  if (channel.allowlist.users.length) {
    parts.push(`${channel.allowlist.users.length} user${channel.allowlist.users.length === 1 ? "" : "s"}`);
  }
  if (channel.allowlist.groups.length) {
    parts.push(`${channel.allowlist.groups.length} group${channel.allowlist.groups.length === 1 ? "" : "s"}`);
  }
  return parts.length ? parts.join(", ") : "none";
}

export function channelStatusTone(status: string): "danger" | "muted" | "ok" | "warning" {
  switch (status) {
    case "ok":
    case "present":
    case "ready":
    case "running":
      return "ok";
    case "blocked":
    case "error":
    case "fail":
    case "missing":
      return "danger";
    case "needs_qr_login":
    case "qr_login_pending":
    case "needs_account":
    case "needs_allow_user":
    case "group_limited":
    case "warn":
      return "warning";
    default:
      return "muted";
  }
}

export function formatChannelName(value: string): string {
  switch (value) {
    case "wechat":
      return "WeChat";
    case "telegram":
      return "Telegram";
    case "feishu":
      return "Feishu";
    case "lark":
      return "Lark";
    default:
      return value;
  }
}
