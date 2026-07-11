import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { ArrowLeft, MessageCircle, Play, Plus, Save, Settings2, Trash2, Wrench, X } from "lucide-react";
import { ActionButton, CreatePanel, Switch } from "@psychevo/components";
import type { ChannelWechatQrPollResult, ChannelWechatQrStartResult, RuntimeProfileView } from "@psychevo/protocol";
import type { SessionBrowserWorkspaceState, WorkbenchChannel, WorkbenchChannelDoctor, WorkbenchChannelSource } from "../types";
import type { ChannelSettingsControls, ChannelUpdateDraft } from "./types";
import {
  CHANNEL_CHOICES,
  CHANNEL_WORKSPACE_MANUAL_VALUE,
  ChannelHealthItem,
  ChannelSetupCard,
  ChannelStatusPill,
  channelAllowlistSummary,
  channelDoctorOk,
  channelDraftFromChannel,
  channelDraftSignature,
  channelModelControlAvailable,
  channelModelOptions,
  channelNativePermissionControlAvailable,
  channelPermissionOptions,
  channelRuntimeProfileOptions,
  channelRuntimeSafetyLabel,
  channelRunnerTone,
  channelRuntimeDefaultsSummary,
  channelRuntimeSummary,
  channelStatusTone,
  channelUpdateDraftFromDraft,
  channelWorkspaceOptionLabel,
  channelWorkspaceOptions,
  channelWorkspaceSelectValue,
  formatChannelName,
  formatRunnerTimestamp,
  modelOptionLabel,
  permissionModeLabel,
  runtimeProfileOptionLabel,
  sectionDomId,
  type ChannelChoice,
  type ChannelSettingsDraft
} from "./channels-support";

export function ChannelsSettingsPanel({
  channelDoctor,
  channels,
  controls,
  disabled,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onLoadChannelSources,
  onPollWechatQrSetup,
  onSetChannelEnabled,
  onStartWechatQrSetup,
  onUpdateChannel,
  runtimeProfiles,
  sessionBrowserWorkspaces,
  cwd
}: {
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  channels: WorkbenchChannel[];
  controls: ChannelSettingsControls;
  disabled: boolean;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  runtimeProfiles: RuntimeProfileView[];
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  cwd: string;
}) {
  const [selectedChannelId, setSelectedChannelId] = useState<string | null>(null);
  const [selectedChannelChoice, setSelectedChannelChoice] = useState<ChannelChoice>("wechat");
  const [setupOpen, setSetupOpen] = useState(false);
  const panelRef = useRef<HTMLElement | null>(null);
  const selectedChannel = channels.find((channel) => channel.id === selectedChannelId) ?? null;
  const configuredChoiceChannel = channels.find((channel) => channel.channel === selectedChannelChoice) ?? null;
  useEffect(() => {
    const settingsContent = panelRef.current?.closest<HTMLElement>(".settingsContent");
    settingsContent?.scrollTo?.({ top: 0, left: 0 });
  }, [selectedChannelId]);
  if (selectedChannel) {
    return (
      <ChannelSettingsDetail
        channel={selectedChannel}
        controls={controls}
        doctor={channelDoctor[selectedChannel.id] ?? null}
        disabled={disabled}
        onBack={() => setSelectedChannelId(null)}
        onDelete={async () => {
          await onDeleteChannel(selectedChannel);
          setSelectedChannelId(null);
        }}
        onLoadSources={() => onLoadChannelSources(selectedChannel)}
        onUpdate={(draft) => onUpdateChannel(selectedChannel, draft)}
        rootRef={panelRef}
        runtimeProfiles={runtimeProfiles}
        sessionBrowserWorkspaces={sessionBrowserWorkspaces}
        cwd={cwd}
      />
    );
  }
  return (
    <section className="agentSurfacePanel channelsSettingsPanel" aria-label="Channels" ref={panelRef}>
      <header className="agentSurfaceHeaderWithAction channelSettingsToolbar">
        <span><MessageCircle size={15} /> Connected Channels <b>{channels.length}</b></span>
        <div className="channelToolbarActions">
          <ActionButton ariaLabel="Doctor Channels" disabled={disabled} icon={<Wrench size={13} />} onClick={onDoctorChannels} tooltip="Doctor Channels" variant="ghost">
            Doctor
          </ActionButton>
          <ActionButton ariaLabel="Set up channel" disabled={disabled} icon={<Plus size={13} />} onClick={() => setSetupOpen(true)} tooltip="Set up channel" variant="primary">
            Set up channel
          </ActionButton>
          <ActionButton ariaLabel="Start Channels" disabled={disabled} icon={<Play size={13} />} onClick={onDoctorChannels} tooltip="Start Channels" variant="ghost">
            Start
          </ActionButton>
        </div>
      </header>
      {setupOpen && (
        <CreatePanel
          className="channelAddSection"
          description="Choose a channel and complete its setup."
          icon={<Plus size={14} />}
          layout="side"
          onClose={() => setSetupOpen(false)}
          title="Set up channel"
        >
          <div className="channelPlatformPicker" role="tablist" aria-label="Channel type">
            {CHANNEL_CHOICES.map((channel) => (
              <button
                aria-selected={selectedChannelChoice === channel}
                className={selectedChannelChoice === channel ? "is-selected" : ""}
                key={channel}
                onClick={() => setSelectedChannelChoice(channel)}
                role="tab"
                type="button"
              >
                {formatChannelName(channel)}
              </button>
            ))}
          </div>
          <ChannelSetupCard
            channel={selectedChannelChoice}
            disabled={disabled}
            existingChannel={configuredChoiceChannel}
            onPollWechatQrSetup={onPollWechatQrSetup}
            onStartWechatQrSetup={onStartWechatQrSetup}
          />
        </CreatePanel>
      )}
      <div className="agentSurfaceList channelSurfaceList">
        {channels.map((channel) => {
          const doctor = channelDoctor[channel.id] ?? null;
          return (
            <div className="agentSurfaceRow channelSurfaceRow" key={channel.id}>
              <div className="channelRowMain">
                <div className="channelIdentityLine">
                  <strong>{channel.label || channel.id}</strong>
                  <ChannelStatusPill status={channel.runtimeStatus} />
                </div>
                <span>{formatChannelName(channel.channel)} · {channel.id} · {channel.transport}</span>
                <div className="channelInlineStates" aria-label={`${channel.id} status`}>
                  <small>Credential {channel.credential.status}</small>
                  <small>Allowlist {channel.allowlist.status}</small>
                  <small>Runner {channel.runner.state}</small>
                  <small>{channelRuntimeSummary(channel, cwd)}</small>
                </div>
                {doctor && (
                  <small className={channelDoctorOk(doctor) ? "agentSurfaceOk" : "agentSurfaceWarning"}>
                    {doctor.checks.map((check) => `${check.name}: ${check.status}`).join(" · ")}
                  </small>
                )}
              </div>
              <div className="agentBackendSide">
                <Switch
                  ariaLabel={`${channel.enabled ? "Disable" : "Enable"} ${channel.id}`}
                  checked={channel.enabled}
                  disabled={disabled}
                  label={channel.enabled ? "Enabled" : "Disabled"}
                  onCheckedChange={(enabled) => onSetChannelEnabled(channel, enabled)}
                  showLabel={false}
                  size="compact"
                />
                <div className="agentBackendActions">
                  <ActionButton ariaLabel={`Test ${channel.id}`} disabled={disabled} icon={<Wrench size={13} />} iconOnly onClick={() => onDoctorChannel(channel)} size="compact" tooltip="Test" variant="ghost">
                    Test {channel.id}
                  </ActionButton>
                  <ActionButton ariaLabel={`Settings ${channel.id}`} disabled={disabled} icon={<Settings2 size={13} />} iconOnly onClick={() => setSelectedChannelId(channel.id)} size="compact" tooltip="Settings" variant="ghost">
                    Settings {channel.id}
                  </ActionButton>
                </div>
              </div>
            </div>
          );
        })}
        {channels.length === 0 && <p>No channels configured.</p>}
      </div>
    </section>
  );
}

function ChannelSettingsDetail({
  channel,
  controls,
  disabled,
  doctor,
  onBack,
  onDelete,
  onLoadSources,
  onUpdate,
  rootRef,
  runtimeProfiles,
  sessionBrowserWorkspaces,
  cwd
}: {
  channel: WorkbenchChannel;
  controls: ChannelSettingsControls;
  disabled: boolean;
  doctor: WorkbenchChannelDoctor | null;
  onBack(): void;
  onDelete(): Promise<void>;
  onLoadSources(): Promise<WorkbenchChannelSource[]>;
  onUpdate(draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  rootRef: RefObject<HTMLElement | null>;
  runtimeProfiles: RuntimeProfileView[];
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  cwd: string;
}) {
  const [draft, setDraft] = useState<ChannelSettingsDraft>(() => channelDraftFromChannel(channel));
  const [discardPrompt, setDiscardPrompt] = useState(false);
  const [deletePrompt, setDeletePrompt] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [sourceLoading, setSourceLoading] = useState(false);
  const [sources, setSources] = useState<WorkbenchChannelSource[] | null>(null);

  useEffect(() => {
    setDraft(channelDraftFromChannel(channel));
    setDiscardPrompt(false);
    setDeletePrompt(false);
    setError(null);
    setSavedNotice(null);
    setSourceError(null);
    setSourceLoading(false);
    setSources(null);
  }, [channel.id]);

  const savedSignature = channelDraftSignature(channelDraftFromChannel(channel));
  const draftSignature = channelDraftSignature(draft);
  const dirty = draftSignature !== savedSignature;
  const permissionOptions = channelPermissionOptions(controls, channel, draft);
  const modelOptions = channelModelOptions(controls, channel, draft);
  const runtimeProfileOptions = channelRuntimeProfileOptions(channel, draft, runtimeProfiles);
  const modelControlAvailable = channelModelControlAvailable(draft.runtimeRef, runtimeProfiles);
  const nativePermissionControlAvailable = channelNativePermissionControlAvailable(draft.runtimeRef, runtimeProfiles);
  const workspaceOptions = channelWorkspaceOptions(sessionBrowserWorkspaces);
  const busy = disabled || saving || deleting;

  function updateDraft(patch: Partial<ChannelSettingsDraft>) {
    setError(null);
    setSavedNotice(null);
    setDiscardPrompt(false);
    setDraft((current) => ({ ...current, ...patch }));
  }

  function requestBack() {
    if (dirty) {
      setDiscardPrompt(true);
      return;
    }
    onBack();
  }

  function cancelEdits() {
    setDraft(channelDraftFromChannel(channel));
    setDiscardPrompt(false);
    setError(null);
    setSavedNotice(null);
  }

  async function saveDraft() {
    if (!dirty || busy) {
      return;
    }
    const workspaceChanged = draft.cwd.trim() !== channelDraftFromChannel(channel).cwd.trim();
    setSaving(true);
    setError(null);
    setSavedNotice(null);
    try {
      const nextChannel = await onUpdate(channelUpdateDraftFromDraft(draft));
      setDraft(channelDraftFromChannel(nextChannel));
      setDiscardPrompt(false);
      setSavedNotice(workspaceChanged ? "Next message will start in the new workspace." : null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  async function deleteChannel() {
    if (busy) {
      return;
    }
    setDeleting(true);
    setError(null);
    try {
      await onDelete();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setDeleting(false);
    }
  }

  async function loadSources() {
    if (sourceLoading || sources !== null) {
      return;
    }
    setSourceLoading(true);
    setSourceError(null);
    try {
      setSources(await onLoadSources());
    } catch (err) {
      setSourceError(err instanceof Error ? err.message : String(err));
    } finally {
      setSourceLoading(false);
    }
  }

  return (
    <section className="channelsSettingsPanel channelDetailPage" aria-label="Channel settings" ref={rootRef}>
      <header className="channelDetailHeader channelDetailHeaderStaged">
        <button aria-label="Back to Channels" onClick={requestBack} title="Back to Channels" type="button">
          <ArrowLeft size={14} />
          <span>Back</span>
        </button>
        <div className="channelDetailTitle">
          <strong>{draft.label.trim() || channel.id}</strong>
          <span>{formatChannelName(channel.channel)} · {channel.transport}</span>
        </div>
        <div className="channelDetailActions">
          {dirty && (
            <button disabled={busy} onClick={cancelEdits} type="button">
              <X size={13} />
              <span>Cancel</span>
            </button>
          )}
          <button
            className="channelDetailSave"
            disabled={busy || !dirty}
            onClick={() => void saveDraft()}
            type="button"
          >
            <Save size={13} />
            <span>{saving ? "Saving" : "Save"}</span>
          </button>
        </div>
      </header>
      {discardPrompt && (
        <div className="channelDiscardNotice" role="alert">
          <strong>Discard unsaved changes?</strong>
          <button disabled={busy} onClick={() => { cancelEdits(); onBack(); }} type="button">
            Discard changes
          </button>
          <button disabled={busy} onClick={() => setDiscardPrompt(false)} type="button">
            Keep editing
          </button>
        </div>
      )}
      {error && <p className="channelDetailError" role="alert">{error}</p>}
      {savedNotice && <p className="channelSavedNotice" role="status">{savedNotice}</p>}
      <div className="channelHealthSummary" aria-label={`${channel.id} channel health summary`}>
        <ChannelHealthItem label="Config" tone={channelStatusTone(channel.runtimeStatus)} value={channel.runtimeStatus} />
        <ChannelHealthItem label="Runner" tone={channelRunnerTone(channel.runner.state)} value={channel.runner.state} />
        <ChannelHealthItem label="Credential" tone={channelStatusTone(channel.credential.status)} value={channel.credential.status} />
        <ChannelHealthItem label="Allowlist" tone={channelStatusTone(channel.allowlist.status)} value={channelAllowlistSummary(channel)} />
        <ChannelHealthItem label="Runtime" tone="muted" value={channelRuntimeDefaultsSummary(draft)} />
      </div>
      {doctor && (
        <div className="channelDoctorResult" role="status" aria-label={`${channel.id} doctor checks`}>
          {doctor.checks.map((check) => (
            <div className={`channelDoctorCheck is-${channelStatusTone(check.status)}`} key={check.name}>
              <span>{check.name}</span>
              <strong>{check.status}</strong>
              <small>{check.message}</small>
            </div>
          ))}
        </div>
      )}
      <div className="channelDetailsForm">
        <ChannelDetailSection title="Connection">
          <ChannelFormRow label="Label" hint="Shown in the connected channel list.">
            <div className="channelControl">
              <input
                aria-label="Channel label"
                disabled={busy}
                onChange={(event) => updateDraft({ label: event.currentTarget.value })}
                type="text"
                value={draft.label}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <ChannelDetailSection title="Access control">
          <ChannelFormRow label="Group mention" hint="Groups only start work after an explicit mention.">
            <label className="channelInlineToggle">
              <input
                aria-label="Require mention in groups"
                checked={draft.requireMention}
                disabled={busy}
                onChange={(event) => updateDraft({ requireMention: event.currentTarget.checked })}
                type="checkbox"
              />
              <span>{draft.requireMention ? "Required" : "Not required"}</span>
            </label>
          </ChannelFormRow>
          <ChannelFormRow label="Allowed callers" hint="Comma or newline separated ids, saved as structured allowlists.">
            <div className="channelTextareaPair">
              <textarea
                aria-label="Allowed direct users"
                disabled={busy}
                onChange={(event) => updateDraft({ allowUsersText: event.currentTarget.value })}
                rows={4}
                value={draft.allowUsersText}
              />
              <textarea
                aria-label="Allowed groups"
                disabled={busy}
                onChange={(event) => updateDraft({ allowGroupsText: event.currentTarget.value })}
                rows={4}
                value={draft.allowGroupsText}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <ChannelDetailSection title="Runtime settings">
          <ChannelFormRow label="Runtime Profile" hint="Blank uses this channel's profile default.">
            <div className="channelControl">
              <select
                aria-label="Channel Runtime Profile"
                disabled={busy}
                onChange={(event) => {
                  const runtimeRef = event.currentTarget.value;
                  updateDraft({
                    runtimeRef,
                    model: channelModelControlAvailable(runtimeRef, runtimeProfiles) ? draft.model : "",
                    permissionMode: channelNativePermissionControlAvailable(runtimeRef, runtimeProfiles)
                      ? draft.permissionMode
                      : "default"
                  });
                }}
                value={draft.runtimeRef}
              >
                {runtimeProfileOptions.map((option) => (
                  <option key={option || "default"} value={option}>{runtimeProfileOptionLabel(option, runtimeProfiles)}</option>
                ))}
              </select>
            </div>
          </ChannelFormRow>
          {nativePermissionControlAvailable ? (
            <ChannelFormRow label="Permission mode" hint="Controls write and command approval defaults for native execution.">
              <div className="channelSegmentedControl" role="group" aria-label="Permission mode">
                {permissionOptions.map((option) => (
                  <button
                    className={draft.permissionMode === option ? "is-selected" : ""}
                    disabled={busy}
                    key={option}
                    onClick={() => updateDraft({ permissionMode: option })}
                    type="button"
                  >
                    {permissionModeLabel(option)}
                  </button>
                ))}
              </div>
            </ChannelFormRow>
          ) : (
            <ChannelFormRow label="Safety policy" hint="Direct execution uses the immutable Runtime Profile policy.">
              <span className="channelFieldHint" aria-label="Runtime Profile safety policy">
                {channelRuntimeSafetyLabel(draft.runtimeRef, runtimeProfiles)}
              </span>
            </ChannelFormRow>
          )}
          {modelControlAvailable ? (
            <ChannelFormRow label="Model" hint="Blank uses the profile default model.">
              <div className="channelControl">
                <select
                  aria-label="Channel model"
                  disabled={busy}
                  onChange={(event) => updateDraft({ model: event.currentTarget.value })}
                  value={draft.model}
                >
                  <option value="">Profile default</option>
                  {modelOptions.map((option) => (
                    <option key={option} value={option}>{modelOptionLabel(option, channel, controls)}</option>
                  ))}
                </select>
              </div>
            </ChannelFormRow>
          ) : (
            <ChannelFormRow label="Model" hint="This Runtime Profile does not declare a Channel-safe model control.">
              <span className="channelFieldHint">Uses runtime default</span>
            </ChannelFormRow>
          )}
          <ChannelFormRow label="Workspace" hint="Changing workspace starts a fresh channel thread on the next message. Current running work is not interrupted.">
            <div className="channelControl channelWorkspaceControl">
              <select
                aria-label="Channel workspace preset"
                disabled={busy}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  if (value !== CHANNEL_WORKSPACE_MANUAL_VALUE) {
                    updateDraft({ cwd: value });
                  }
                }}
                value={channelWorkspaceSelectValue(draft.cwd, workspaceOptions)}
              >
                <option value="">Profile default</option>
                {workspaceOptions.map((path) => (
                  <option key={path} value={path}>{channelWorkspaceOptionLabel(path)}</option>
                ))}
                {channelWorkspaceSelectValue(draft.cwd, workspaceOptions) === CHANNEL_WORKSPACE_MANUAL_VALUE && (
                  <option value={CHANNEL_WORKSPACE_MANUAL_VALUE}>Manual path</option>
                )}
              </select>
              <input
                aria-label="Channel workspace"
                disabled={busy}
                onChange={(event) => updateDraft({ cwd: event.currentTarget.value })}
                placeholder={cwd}
                type="text"
                value={draft.cwd}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <details
          className="channelAdvancedDiagnostics"
          onToggle={(event) => {
            if (event.currentTarget.open) {
              void loadSources();
            }
          }}
        >
          <summary>Advanced diagnostics</summary>
          <ChannelCredentialRow
            disabled={busy}
            hint="Advanced credential-name override for custom or manual setups. Raw secrets stay in the profile environment."
            label="Credential env"
            onChange={(value) => updateDraft({ credentialEnv: value })}
            status={channel.credential.status}
            value={draft.credentialEnv}
          />
          <ChannelFormRow label="Runner activity" hint="Diagnostics are read-only and secret-free.">
            <dl className="channelRunnerGrid">
              <div><dt>State</dt><dd>{channel.runner.state}</dd></div>
              <div><dt>Reason</dt><dd>{channel.runner.reason ?? "none"}</dd></div>
              <div><dt>Last poll</dt><dd>{formatRunnerTimestamp(channel.runner.lastPollAtMs)}</dd></div>
              <div><dt>Last healthy poll</dt><dd>{formatRunnerTimestamp(channel.runner.lastHealthyPollAtMs)}</dd></div>
              <div><dt>Last inbound</dt><dd>{formatRunnerTimestamp(channel.runner.lastInboundAtMs)}</dd></div>
              <div><dt>Last outbound</dt><dd>{formatRunnerTimestamp(channel.runner.lastOutboundAtMs)}</dd></div>
              <div><dt>Last iLink code</dt><dd>{channel.runner.lastIlinkErrcode == null ? "none" : String(channel.runner.lastIlinkErrcode)}</dd></div>
              <div><dt>Last error</dt><dd>{channel.runner.lastError ?? "none"}</dd></div>
            </dl>
          </ChannelFormRow>
          <ChannelFormRow label="Remote lanes" hint="Shows which remote chats are bound to local threads.">
            <ChannelSourceList
              error={sourceError}
              loading={sourceLoading}
              sources={sources}
            />
          </ChannelFormRow>
        </details>

        <section className="channelDangerSection" aria-labelledby="channel-danger-heading">
          <div>
            <h4 id="channel-danger-heading">Danger zone</h4>
            <span>Remove this channel config. .env secret values are left intact.</span>
          </div>
          <div className="channelDangerActions">
            {deletePrompt ? (
              <>
                <button disabled={busy} onClick={() => void deleteChannel()} type="button">
                  {deleting ? "Removing" : "Confirm remove"}
                </button>
                <button disabled={busy} onClick={() => setDeletePrompt(false)} type="button">
                  Cancel
                </button>
              </>
            ) : (
              <button disabled={busy} onClick={() => setDeletePrompt(true)} type="button">
                <Trash2 size={13} />
                <span>Remove channel</span>
              </button>
            )}
          </div>
        </section>
      </div>
    </section>
  );
}
function ChannelDetailSection({
  children,
  title
}: {
  children: ReactNode;
  title: string;
}) {
  return (
    <section className="channelDetailSection" aria-labelledby={`channel-section-${sectionDomId(title)}`}>
      <h4 id={`channel-section-${sectionDomId(title)}`}>{title}</h4>
      {children}
    </section>
  );
}

function ChannelFormRow({
  children,
  hint,
  label
}: {
  children: ReactNode;
  hint: string;
  label: string;
}) {
  return (
    <div className="channelFormRow">
      <div className="channelFormCopy">
        <strong>{label}</strong>
        <span>{hint}</span>
      </div>
      {children}
    </div>
  );
}

function ChannelCredentialRow({
  disabled,
  hint,
  label,
  onChange,
  status,
  value
}: {
  disabled: boolean;
  hint: string;
  label: string;
  onChange(value: string): void;
  status: string;
  value: string;
}) {
  return (
    <ChannelFormRow label={label} hint={hint}>
      <div className="channelCredentialControl">
        <div className="channelCredentialLine">
          <code>{value.trim() || "platform default"}</code>
          <small className={`channelCredentialStatus is-${channelStatusTone(status)}`}>{status}</small>
        </div>
        <input
          aria-label={label}
          disabled={disabled}
          onChange={(event) => onChange(event.currentTarget.value)}
          type="text"
          value={value}
        />
      </div>
    </ChannelFormRow>
  );
}

function ChannelSourceList({
  error,
  loading,
  sources
}: {
  error: string | null;
  loading: boolean;
  sources: WorkbenchChannelSource[] | null;
}) {
  if (loading && sources === null) {
    return <p className="channelSourceEmpty">Loading remote lanes...</p>;
  }
  if (error) {
    return <p className="channelDetailError" role="alert">{error}</p>;
  }
  if (!sources || sources.length === 0) {
    return <p className="channelSourceEmpty">No remote lanes have started a local thread yet.</p>;
  }
  return (
    <div className="channelSourceList">
      {sources.map((source) => (
        <div className="channelSourceItem" key={source.sourceKey}>
          <div>
            <strong>{source.visibleName ?? `${source.platform} channel lane`}</strong>
            <span>{source.threadTitle ?? source.threadId}</span>
          </div>
          <dl>
            <div><dt>Workspace</dt><dd>{source.cwd || "unknown"}</dd></div>
            <div><dt>Activity</dt><dd>{source.activityStatus}{source.queuedTurns > 0 ? ` (${source.queuedTurns} queued)` : ""}</dd></div>
            <div><dt>Chat</dt><dd>{source.chatLabel ?? "unknown"}</dd></div>
            <div><dt>User</dt><dd>{source.userLabel ?? "unknown"}</dd></div>
          </dl>
        </div>
      ))}
    </div>
  );
}
