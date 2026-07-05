import { useEffect, useId, useMemo, useState, type FormEvent } from "react";
import { Edit3, Keyboard, Link2, Plus, RotateCcw, Save, Trash2, X } from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type { SlashSettingsResult } from "@psychevo/protocol";
import { ActionButton } from "@psychevo/components";
import { errorMessage } from "./common";

type SlashAliasDraft = {
  originalAlias: string | null;
  alias: string;
  target: string;
};

type SlashKeybindDraft = {
  originalShortcut: string | null;
  shortcut: string;
  target: string;
};

type SlashTargetGroup = {
  target: string;
  summary: string | null;
  aliases: SlashSettingsResult["aliases"];
  keybinds: SlashSettingsResult["keybinds"];
};

const EMPTY_SLASH_ALIAS_DRAFT: SlashAliasDraft = { originalAlias: null, alias: "", target: "" };
const EMPTY_SLASH_KEYBIND_DRAFT: SlashKeybindDraft = { originalShortcut: null, shortcut: "", target: "" };

export function SlashCommandsSettingsPanel({
  client,
  disabled,
  onSaved,
  cwd
}: {
  client: GatewayClient | null;
  disabled: boolean;
  onSaved(): Promise<void>;
  cwd: string;
}) {
  const [settings, setSettings] = useState<SlashSettingsResult | null>(null);
  const [aliasDraft, setAliasDraft] = useState<SlashAliasDraft>(EMPTY_SLASH_ALIAS_DRAFT);
  const [keybindDraft, setKeybindDraft] = useState<SlashKeybindDraft>(EMPTY_SLASH_KEYBIND_DRAFT);
  const [leaderKeyDraft, setLeaderKeyDraft] = useState("ctrl+x");
  const [leaderTimeoutDraft, setLeaderTimeoutDraft] = useState("2000");
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const targetListId = useId();

  async function loadSlashSettings() {
    if (!client) {
      setSettings(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await client.request("slash/settings/read", {
        scope: "global",
        cwd: cwd
      });
      applySlashSettings(result);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadSlashSettings();
  }, [client, cwd]);

  function applySlashSettings(result: SlashSettingsResult) {
    setSettings(result);
    setLeaderKeyDraft(result.leaderKey);
    setLeaderTimeoutDraft(String(result.leaderTimeoutMs));
  }

  async function saveSlashSettings(
    next: Partial<Pick<SlashSettingsResult, "aliases" | "keybinds" | "leaderKey" | "leaderTimeoutMs">>,
    busy: string,
    message: string
  ) {
    if (!client || !settings) return;
    setBusyKey(busy);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("slash/settings/update", {
        scope: "global",
        cwd: cwd,
        leaderKey: next.leaderKey ?? settings.leaderKey,
        leaderTimeoutMs: next.leaderTimeoutMs ?? settings.leaderTimeoutMs,
        aliases: (next.aliases ?? settings.aliases).map((entry) => ({
          alias: entry.alias,
          target: entry.target,
          targetSummary: null
        })),
        keybinds: (next.keybinds ?? settings.keybinds).map((entry) => ({
          shortcut: entry.shortcut,
          target: entry.target,
          targetSummary: null
        }))
      });
      applySlashSettings(result);
      setNotice(message);
      await onSaved();
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  function saveLeader(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const timeout = Number.parseInt(leaderTimeoutDraft, 10);
    if (!Number.isFinite(timeout) || timeout <= 0) {
      setError("Leader timeout must be a positive integer.");
      return;
    }
    void saveSlashSettings(
      {
        leaderKey: leaderKeyDraft.trim(),
        leaderTimeoutMs: timeout
      },
      "leader",
      "Leader shortcut saved"
    );
  }

  function saveAlias(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!settings) return;
    const alias = aliasDraft.alias.trim();
    const target = aliasDraft.target.trim();
    const nextAliases = settings.aliases
      .filter((entry) => entry.alias !== aliasDraft.originalAlias)
      .concat({ alias, target, targetSummary: null });
    void saveSlashSettings({ aliases: nextAliases }, "alias", "Alias saved");
    setAliasDraft(EMPTY_SLASH_ALIAS_DRAFT);
  }

  function saveKeybind(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!settings) return;
    const shortcut = keybindDraft.shortcut.trim();
    const target = keybindDraft.target.trim();
    const nextKeybinds = settings.keybinds
      .filter((entry) => entry.shortcut !== keybindDraft.originalShortcut)
      .concat({ shortcut, target, targetSummary: null });
    void saveSlashSettings({ keybinds: nextKeybinds }, "keybind", "Shortcut saved");
    setKeybindDraft(EMPTY_SLASH_KEYBIND_DRAFT);
  }

  const groups = useMemo(() => slashTargetGroups(settings), [settings]);
  const targetOptions = useMemo(() => groups.map((group) => group.target), [groups]);
  const totalRows = (settings?.aliases.length ?? 0) + (settings?.keybinds.length ?? 0);
  const saving = Boolean(busyKey);

  return (
    <section className="agentSurfacePanel slashSettingsPanel" aria-label="Slash Commands">
      <header className="agentSurfaceHeaderWithAction">
        <span><Keyboard size={15} /> Profile Slash Commands <b>{totalRows}</b></span>
        <ActionButton
          aria-label="Refresh slash command settings"
          disabled={disabled || loading || !client}
          icon={<RotateCcw size={13} />}
          iconOnly
          onClick={() => void loadSlashSettings()}
          size="compact"
          title="Refresh"
          type="button"
          variant="ghost"
        >
          Refresh
        </ActionButton>
      </header>
      {error && <div className="modelSettingsMessage is-error" role="alert">{error}</div>}
      {notice && <div className="modelSettingsMessage">{notice}</div>}
      {(settings?.diagnostics ?? []).map((diagnostic) => (
        <div className="modelSettingsMessage is-warning" key={diagnostic}>{diagnostic}</div>
      ))}
      <datalist id={targetListId}>
        {targetOptions.map((target) => <option key={target} value={target} />)}
      </datalist>
      <div className="slashSettingsLayout">
        <div className="slashSettingsMain">
          <form className="slashLeaderForm slashSettingsCard" onSubmit={saveLeader}>
            <div className="slashSettingsCardCopy">
              <strong>Leader shortcut</strong>
              <span>Controls TUI keybind sequences such as <code>&lt;leader&gt;s</code>.</span>
            </div>
            <div className="slashLeaderControls">
              <label>
                <span>Leader key</span>
                <input
                  disabled={disabled || !client || loading || saving}
                  onChange={(event) => setLeaderKeyDraft(event.currentTarget.value)}
                  placeholder="ctrl+x"
                  value={leaderKeyDraft}
                />
              </label>
              <label>
                <span>Timeout ms</span>
                <input
                  disabled={disabled || !client || loading || saving}
                  inputMode="numeric"
                  onChange={(event) => setLeaderTimeoutDraft(event.currentTarget.value)}
                  placeholder="2000"
                  value={leaderTimeoutDraft}
                />
              </label>
              <ActionButton disabled={disabled || !client || loading || saving} icon={<Save size={13} />} title="Save leader key" type="submit" variant="neutral">
                {busyKey === "leader" ? "Saving" : "Save"}
              </ActionButton>
            </div>
          </form>

          <section className="slashSettingsCard slashCommandCatalog" aria-label="Configured slash command targets">
            <header className="slashCommandSectionHeader">
              <div>
                <h4>Configured targets</h4>
                <p>Aliases and shortcuts are grouped by the slash line they run.</p>
              </div>
              <span>{groups.length} targets</span>
            </header>
            <div className="agentSurfaceList slashCommandList">
              {groups.map((group) => (
                <div className="agentSurfaceRow slashCommandRow" key={group.target}>
                  <div className="slashCommandMain">
                    <div className="slashCommandTarget">
                      <strong><code>{group.target}</code></strong>
                      <div className="slashCommandStats" aria-label="Slash target mappings">
                        <span>{formatCount(group.aliases.length, "alias")}</span>
                        <span>{formatCount(group.keybinds.length, "shortcut")}</span>
                      </div>
                    </div>
                    <span>{group.summary ?? "Slash target"}</span>
                    <div className="slashChipLine">
                      {group.aliases.map((entry) => (
                        <span className="slashChip" key={`alias:${entry.alias}`}>
                          <code>{entry.alias}</code>
                          <button
                            aria-label={`Edit alias ${entry.alias}`}
                            disabled={disabled || saving}
                            onClick={() => setAliasDraft({ originalAlias: entry.alias, alias: entry.alias, target: entry.target })}
                            title="Edit alias"
                            type="button"
                          >
                            <Edit3 size={11} />
                          </button>
                          <button
                            aria-label={`Delete alias ${entry.alias}`}
                            disabled={disabled || saving || !settings}
                            onClick={() => settings && void saveSlashSettings({
                              aliases: settings.aliases.filter((alias) => alias.alias !== entry.alias)
                            }, `alias:${entry.alias}`, "Alias deleted")}
                            title="Delete alias"
                            type="button"
                          >
                            <Trash2 size={11} />
                          </button>
                        </span>
                      ))}
                      {group.keybinds.map((entry) => (
                        <span className="slashChip is-shortcut" key={`keybind:${entry.shortcut}`}>
                          <code>{entry.shortcut}</code>
                          <button
                            aria-label={`Edit shortcut ${entry.shortcut}`}
                            disabled={disabled || saving}
                            onClick={() => setKeybindDraft({ originalShortcut: entry.shortcut, shortcut: entry.shortcut, target: entry.target })}
                            title="Edit shortcut"
                            type="button"
                          >
                            <Edit3 size={11} />
                          </button>
                          <button
                            aria-label={`Delete shortcut ${entry.shortcut}`}
                            disabled={disabled || saving || !settings}
                            onClick={() => settings && void saveSlashSettings({
                              keybinds: settings.keybinds.filter((keybind) => keybind.shortcut !== entry.shortcut)
                            }, `keybind:${entry.shortcut}`, "Shortcut deleted")}
                            title="Delete shortcut"
                            type="button"
                          >
                            <Trash2 size={11} />
                          </button>
                        </span>
                      ))}
                    </div>
                  </div>
                  <div className="slashCommandRowActions">
                    <button
                      disabled={disabled || saving}
                      onClick={() => setAliasDraft({ originalAlias: null, alias: "", target: group.target })}
                      type="button"
                    >
                      <Plus size={12} aria-hidden /> Alias
                    </button>
                    <button
                      disabled={disabled || saving}
                      onClick={() => setKeybindDraft({ originalShortcut: null, shortcut: "", target: group.target })}
                      type="button"
                    >
                      <Plus size={12} aria-hidden /> Shortcut
                    </button>
                  </div>
                </div>
              ))}
              {!loading && groups.length === 0 && (
                <div className="slashEmptyState">
                  <strong>No custom mappings</strong>
                  <span>Add an alias or shortcut to make frequent slash commands faster.</span>
                </div>
              )}
              {loading && <p>Loading slash command settings...</p>}
            </div>
          </section>
        </div>

        <aside className="slashSettingsSide" aria-label="Slash command editors">
          <section className="slashSettingsCard slashEditorPanel">
            <header className="slashCommandSectionHeader">
              <div>
                <h4>Create mappings</h4>
                <p>Point aliases and shortcuts at any complete slash line.</p>
              </div>
              <Link2 size={14} aria-hidden />
            </header>
            <div className="slashEditorGrid">
              <form className="backendEditor slashCommandEditor" onSubmit={saveAlias}>
                <header>
                  <h4>{aliasDraft.originalAlias ? "Edit alias" : "Add alias"}</h4>
                  {aliasDraft.originalAlias && (
                    <button aria-label="Cancel alias edit" onClick={() => setAliasDraft(EMPTY_SLASH_ALIAS_DRAFT)} title="Cancel" type="button">
                      <X size={14} />
                    </button>
                  )}
                </header>
                <label>
                  <span>Alias</span>
                  <input
                    disabled={disabled || !client || loading || saving}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setAliasDraft((current) => ({ ...current, alias: value }));
                    }}
                    placeholder="/st"
                    value={aliasDraft.alias}
                  />
                </label>
                <label>
                  <span>Target slash line</span>
                  <input
                    disabled={disabled || !client || loading || saving}
                    list={targetListId}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setAliasDraft((current) => ({ ...current, target: value }));
                    }}
                    placeholder="/status"
                    value={aliasDraft.target}
                  />
                </label>
                <footer>
                  <ActionButton disabled={disabled || !client || loading || saving || !aliasDraft.alias.trim() || !aliasDraft.target.trim()} icon={<Save size={13} />} type="submit" variant="primary">
                    {busyKey === "alias" ? "Saving" : "Save alias"}
                  </ActionButton>
                </footer>
              </form>
              <form className="backendEditor slashCommandEditor" onSubmit={saveKeybind}>
                <header>
                  <h4>{keybindDraft.originalShortcut ? "Edit shortcut" : "Add shortcut"}</h4>
                  {keybindDraft.originalShortcut && (
                    <button aria-label="Cancel shortcut edit" onClick={() => setKeybindDraft(EMPTY_SLASH_KEYBIND_DRAFT)} title="Cancel" type="button">
                      <X size={14} />
                    </button>
                  )}
                </header>
                <label>
                  <span>Shortcut</span>
                  <input
                    disabled={disabled || !client || loading || saving}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setKeybindDraft((current) => ({ ...current, shortcut: value }));
                    }}
                    placeholder="<leader>s"
                    value={keybindDraft.shortcut}
                  />
                </label>
                <label>
                  <span>Target slash line</span>
                  <input
                    disabled={disabled || !client || loading || saving}
                    list={targetListId}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setKeybindDraft((current) => ({ ...current, target: value }));
                    }}
                    placeholder="/status"
                    value={keybindDraft.target}
                  />
                </label>
                <footer>
                  <ActionButton disabled={disabled || !client || loading || saving || !keybindDraft.shortcut.trim() || !keybindDraft.target.trim()} icon={<Save size={13} />} type="submit" variant="primary">
                    {busyKey === "keybind" ? "Saving" : "Save shortcut"}
                  </ActionButton>
                </footer>
              </form>
            </div>
          </section>
        </aside>
      </div>
    </section>
  );
}

function slashTargetGroups(settings: SlashSettingsResult | null): SlashTargetGroup[] {
  if (!settings) {
    return [];
  }
  const groups = new Map<string, SlashTargetGroup>();
  for (const entry of settings.aliases) {
    const group = ensureSlashTargetGroup(groups, entry.target, entry.targetSummary);
    group.aliases.push(entry);
  }
  for (const entry of settings.keybinds) {
    const group = ensureSlashTargetGroup(groups, entry.target, entry.targetSummary);
    group.keybinds.push(entry);
  }
  return [...groups.values()].sort((left, right) => left.target.localeCompare(right.target));
}

function ensureSlashTargetGroup(
  groups: Map<string, SlashTargetGroup>,
  target: string,
  summary: string | null
): SlashTargetGroup {
  const existing = groups.get(target);
  if (existing) {
    if (!existing.summary && summary) {
      existing.summary = summary;
    }
    return existing;
  }
  const next: SlashTargetGroup = { target, summary, aliases: [], keybinds: [] };
  groups.set(target, next);
  return next;
}

function formatCount(count: number, label: string): string {
  return `${count} ${label}${count === 1 ? "" : "s"}`;
}
