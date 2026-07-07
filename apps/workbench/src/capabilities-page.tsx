import { useEffect, useMemo, useState } from "react";
import type { GatewayClient } from "@psychevo/client";
import { ActionButton, CreatePanel, MarkdownText, Switch } from "@psychevo/components";
import type { GatewayRequestScope } from "@psychevo/protocol";
import { LogIn, LogOut, Play, Plus, RefreshCw, Search, Trash2 } from "lucide-react";

type CapabilityTab = "skills" | "plugins" | "mcp" | "tools";
type JsonObject = Record<string, unknown>;

type CapabilityRow = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  status: string;
  badges: string[];
  raw: JsonObject;
};

type SkillInstallDraft = { source: string; name: string; target: "profile" | "project"; force: boolean };
type PluginInstallDraft = {
  source: string;
  kind: "local" | "git" | "npm";
  npmVersion: string;
  npmRegistry: string;
  adapterMode: "manifest_only" | "adapter_host" | "disabled";
  force: boolean;
  inspection: JsonObject | null;
};
type MutationOptions = { notice?: string; refresh?: boolean };

type SkillRow = CapabilityRow & {
  collisionGroup: string[];
  issues: string[];
  location: string;
  missingCredentialFiles: string[];
  missingEnvVars: string[];
  promptVisible: boolean;
  readiness: string;
  requiredTools: string[];
  requiredToolsets: string[];
  skillDir: string;
  source: string;
  sourceLabel: string;
  supported: boolean;
  tags: string[];
};

const TABS: Array<{ id: CapabilityTab; label: string }> = [
  { id: "skills", label: "Skills" },
  { id: "plugins", label: "Plugins" },
  { id: "mcp", label: "MCP" },
  { id: "tools", label: "Tools" }
];

export function CapabilitiesPage({
  client,
  cwd,
  disabled,
  onCopyText,
  scope
}: {
  client: GatewayClient | null;
  cwd: string;
  disabled: boolean;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  scope: GatewayRequestScope | null;
}) {
  const [activeTab, setActiveTab] = useState<CapabilityTab>("skills");
  const [query, setQuery] = useState("");
  const [data, setData] = useState<Record<CapabilityTab, JsonObject | null>>({
    skills: null,
    plugins: null,
    mcp: null,
    tools: null
  });
  const [selected, setSelected] = useState<Record<CapabilityTab, string | null>>({
    skills: null,
    plugins: null,
    mcp: null,
    tools: null
  });
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [createPanel, setCreatePanel] = useState<CapabilityTab | null>(null);
  const [skillInstall, setSkillInstall] = useState<SkillInstallDraft>({ source: "", name: "", target: "profile", force: false });
  const [pluginInstall, setPluginInstall] = useState<PluginInstallDraft>({ source: "", kind: "local", npmVersion: "", npmRegistry: "", adapterMode: "manifest_only", force: false, inspection: null });
  const [toolDraft, setToolDraft] = useState({ name: "", description: "", tools: "", includes: "", force: false });
  const [mcpDraft, setMcpDraft] = useState({
    name: "",
    transport: "stdio",
    command: "",
    url: "",
    bearerTokenEnvVar: "",
    oauthClientId: ""
  });
  const [toolPolicyDraft, setToolPolicyDraft] = useState({ enabledTools: "", disabledTools: "" });
  const [oauthSession, setOauthSession] = useState<string | null>(null);

  const requestScope = scope ?? (cwd ? { cwd, source: { kind: "web", rawId: null, lifetime: "persistent", rawIdentity: null, visibleName: null } } as GatewayRequestScope : null);

  useEffect(() => {
    if (!client || !requestScope) return;
    const activeClient = client;
    const activeScope = requestScope;
    let cancelled = false;
    async function load() {
      setLoading(true);
      setError(null);
      try {
        const result = await requestTab(activeClient, activeTab, activeScope);
        if (cancelled) return;
        setData((current) => ({ ...current, [activeTab]: objectValue(result) }));
      } catch (err) {
        if (!cancelled) setError(errorMessage(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [activeTab, client, refreshToken, requestScope?.cwd]);

  useEffect(() => {
    if (!client || !requestScope || !oauthSession) return;
    let stopped = false;
    const timer = window.setInterval(() => {
      void client.request("mcp/oauth/status", { sessionId: oauthSession, scope: requestScope }).then((result) => {
        const status = stringField(result, "status");
        if (stopped || status === "pending") return;
        window.clearInterval(timer);
        setOauthSession(null);
        setNotice(status === "succeeded" ? "OAuth login saved. Changes apply to the next run/session." : stringField(result, "error") || "OAuth login failed.");
        setRefreshToken((value) => value + 1);
      }).catch((err) => {
        if (!stopped) {
          window.clearInterval(timer);
          setOauthSession(null);
          setError(errorMessage(err));
        }
      });
    }, 1200);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [client, oauthSession, requestScope?.cwd]);

  const rows = useMemo(() => {
    const source = data[activeTab];
    const all = rowsForTab(activeTab, source);
    const needle = query.trim().toLowerCase();
    if (!needle) return all;
    return all.filter((row) => `${row.name} ${row.description}`.toLowerCase().includes(needle));
  }, [activeTab, data, query]);

  const selectedId = selected[activeTab] ?? rows[0]?.id ?? null;
  const selectedRow = rows.find((row) => row.id === selectedId) ?? rows[0] ?? null;

  async function mutate(action: () => Promise<unknown>, options: MutationOptions = {}): Promise<boolean> {
    if (!client || !requestScope) return false;
    setSaving(true);
    setError(null);
    try {
      await action();
      setNotice(options.notice ?? "Saved. Changes apply to the next run/session; current sessions may differ.");
      if (options.refresh !== false) setRefreshToken((value) => value + 1);
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    } finally {
      setSaving(false);
    }
  }

  const busy = disabled || loading || saving || !client || !requestScope;

  return (
    <section aria-label="Capabilities" className="capabilitiesPage">
      <header className="capabilitiesHeader">
        <div>
          <h2>Capabilities</h2>
          <span>{cwd}</span>
        </div>
        <ActionButton ariaLabel="Refresh" disabled={busy} icon={<RefreshCw size={15} />} iconOnly onClick={() => setRefreshToken((value) => value + 1)} tooltip="Refresh" variant="ghost">
          Refresh
        </ActionButton>
      </header>

      <div className="capabilityTabs" role="tablist" aria-label="Capability types">
        {TABS.map((tab) => (
          <button
            aria-selected={activeTab === tab.id}
            className={activeTab === tab.id ? "is-selected" : ""}
            key={tab.id}
            onClick={() => {
              setActiveTab(tab.id);
              setCreatePanel(null);
              setQuery("");
            }}
            role="tab"
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div className="capabilitiesToolbar">
        <label>
          <Search size={15} />
          <input aria-label={`Search ${tabLabel(activeTab)}`} onChange={(event) => setQuery(event.target.value)} placeholder="Search" value={query} />
        </label>
        <ActionButton
          active={createPanel === activeTab}
          disabled={busy}
          icon={<Plus size={14} />}
          onClick={() => setCreatePanel((current) => current === activeTab ? null : activeTab)}
          variant={createPanel === activeTab ? "neutral" : "primary"}
        >
          {createActionLabel(activeTab)}
        </ActionButton>
      </div>

      {(error || notice || oauthSession) && (
        <div className={`capabilityBanner ${error ? "is-error" : ""}`}>
          {error ?? (oauthSession ? "OAuth login pending" : notice)}
        </div>
      )}

      {activeTab === "skills" ? (
        <SkillsPanel
          busy={busy}
          client={client}
          data={data.skills}
          loading={loading}
          mutate={mutate}
          onCopyText={onCopyText}
          query={query}
          refreshToken={refreshToken}
          scope={requestScope}
          createOpen={createPanel === "skills"}
          selectedId={selected.skills}
          setSkillInstall={setSkillInstall}
          skillInstall={skillInstall}
          onCloseCreate={() => setCreatePanel(null)}
          onSelect={(id) => setSelected((current) => ({ ...current, skills: id }))}
        />
      ) : (
        <>
          <CapabilityForms
            busy={busy}
            client={client}
            scope={requestScope}
            tab={activeTab}
            pluginInstall={pluginInstall}
            setPluginInstall={setPluginInstall}
            toolDraft={toolDraft}
            setToolDraft={setToolDraft}
            mcpDraft={mcpDraft}
            setMcpDraft={setMcpDraft}
            mutate={mutate}
            open={createPanel === activeTab}
            onClose={() => setCreatePanel(null)}
          />

          <div className="capabilitiesGrid">
            <div className="capabilityList" role="list">
              {loading && <div className="capabilityEmpty">Loading</div>}
              {!loading && rows.length === 0 && <div className="capabilityEmpty">No matches</div>}
              {rows.map((row) => {
                const selectedClass = row.id === selectedRow?.id ? " is-selected" : "";
                if (hasCapabilityRowSwitch(activeTab)) {
                  return (
                    <div className={`capabilityRow capabilityRowWithSwitch${selectedClass}`} key={row.id} role="listitem">
                      <button
                        aria-label={`${rowKindLabel(activeTab)} ${row.name}`}
                        className="capabilityRowSelect"
                        onClick={() => setSelected((current) => ({ ...current, [activeTab]: row.id }))}
                        type="button"
                      >
                        <span className="capabilityRowMain">
                          <strong>{row.name}</strong>
                          <small>{row.description || row.status}</small>
                        </span>
                        <CapabilityBadges row={row} />
                      </button>
                      <Switch
                        ariaLabel={row.enabled ? `Disable ${row.name}` : `Enable ${row.name}`}
                        checked={row.enabled}
                        className="capabilityRowSwitch"
                        disabled={busy}
                        label={row.enabled ? "Enabled" : "Disabled"}
                        onCheckedChange={(enabled) => void mutate(() => setCapabilityEnabled(client, requestScope, activeTab, row, enabled))}
                        showLabel={false}
                        size="compact"
                      />
                    </div>
                  );
                }
                return (
                  <button
                    className={`capabilityRow${selectedClass}`}
                    key={row.id}
                    onClick={() => setSelected((current) => ({ ...current, [activeTab]: row.id }))}
                    type="button"
                  >
                    <span className="capabilityRowMain">
                      <strong>{row.name}</strong>
                      <small>{row.description || row.status}</small>
                    </span>
                    <span className="capabilityRowMeta">
                      <span className={row.enabled ? "capabilityChip is-on" : "capabilityChip"}>{row.enabled ? "On" : "Off"}</span>
                      {row.badges.slice(0, 2).map((badge) => <span className="capabilityChip" key={badge}>{badge}</span>)}
                    </span>
                  </button>
                );
              })}
            </div>

            <aside className="capabilityDetail" aria-label={`${tabLabel(activeTab)} detail`}>
              {selectedRow ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{selectedRow.name}</h3>
                      <span>{selectedRow.status}</span>
                    </div>
                  </div>
                  <CapabilityActions
                    busy={busy}
                    client={client}
                    row={selectedRow}
                    scope={requestScope}
                    tab={activeTab}
                    toolPolicyDraft={toolPolicyDraft}
                    setToolPolicyDraft={setToolPolicyDraft}
                    mutate={mutate}
                    onOAuthSession={setOauthSession}
                  />
                  <KeyValueView value={selectedRow.raw} />
                </>
              ) : (
                <div className="capabilityEmpty">Select an item</div>
              )}
            </aside>
          </div>
        </>
      )}
    </section>
  );
}

function SkillsPanel({
  busy,
  client,
  createOpen,
  data,
  loading,
  mutate,
  onCloseCreate,
  onCopyText,
  onSelect,
  query,
  refreshToken,
  scope,
  selectedId,
  setSkillInstall,
  skillInstall
}: {
  busy: boolean;
  client: GatewayClient | null;
  createOpen: boolean;
  data: JsonObject | null;
  loading: boolean;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onCloseCreate(): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onSelect(id: string): void;
  query: string;
  refreshToken: number;
  scope: GatewayRequestScope | null;
  selectedId: string | null;
  setSkillInstall(value: SkillInstallDraft): void;
  skillInstall: SkillInstallDraft;
}) {
  const [detail, setDetail] = useState<{ id: string; loading: boolean; value: JsonObject | null; error: string | null } | null>(null);

  const allRows = useMemo(() => skillRowsFromData(data), [data]);
  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return allRows.filter((row) => !needle || `${row.name} ${row.description}`.toLowerCase().includes(needle));
  }, [allRows, query]);

  const activeId = selectedId && rows.some((row) => row.id === selectedId) ? selectedId : rows[0]?.id ?? null;
  const row = rows.find((item) => item.id === activeId) ?? null;

  useEffect(() => {
    if (!client || !scope || !row) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetail({ id: row.id, loading: true, value: null, error: null });
    void client.request("skill/read", { name: row.name, path: row.location || null, scope }).then((value) => {
      if (!cancelled) setDetail({ id: row.id, loading: false, value: objectValue(value), error: null });
    }).catch((err) => {
      if (!cancelled) setDetail({ id: row.id, loading: false, value: null, error: errorMessage(err) });
    });
    return () => {
      cancelled = true;
    };
  }, [client, refreshToken, row?.id, row?.location, scope?.cwd]);

  if (!client || !scope) {
    return <div className="capabilityEmpty">Gateway unavailable</div>;
  }

  return (
    <>
      {createOpen && (
        <CreatePanel className="capabilityCreatePanel" description="Install a skill from a local path or Git source." icon={<Plus size={14} />} layout="side" onClose={onCloseCreate} title="Install skill">
          <form className="capabilityForm skillInstallForm" onSubmit={(event) => {
            event.preventDefault();
            if (skillInstall.force && !confirmAction("Install skill with force?")) return;
            void mutate(() => client.request("skill/install", {
              source: skillInstall.source,
              name: skillInstall.name || null,
              target: skillInstall.target,
              force: skillInstall.force,
              scope
            })).then((ok) => {
              if (ok) onCloseCreate();
            });
          }}>
            <input aria-label="Skill source" onChange={(event) => setSkillInstall({ ...skillInstall, source: event.target.value })} placeholder="path or git source" value={skillInstall.source} />
            <input aria-label="Skill name" onChange={(event) => setSkillInstall({ ...skillInstall, name: event.target.value })} placeholder="name" value={skillInstall.name} />
            <select aria-label="Skill target" onChange={(event) => setSkillInstall({ ...skillInstall, target: event.target.value === "project" ? "project" : "profile" })} value={skillInstall.target}>
              <option value="profile">Profile</option>
              <option value="project">Project</option>
            </select>
            <label><input checked={skillInstall.force} onChange={(event) => setSkillInstall({ ...skillInstall, force: event.target.checked })} type="checkbox" /> Force</label>
            <ActionButton disabled={busy || !skillInstall.source.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Install</ActionButton>
          </form>
        </CreatePanel>
      )}

      <div className="capabilitiesGrid skillsGrid">
        <div className="capabilityList" role="list">
          {loading && <div className="capabilityEmpty">Loading</div>}
          {!loading && rows.length === 0 && <div className="capabilityEmpty">No matches</div>}
          {rows.map((item) => {
            const metadata = skillRowMetadata(item);
            return (
              <div
                className={item.id === row?.id ? "capabilityRow skillRow capabilityRowWithSwitch is-selected" : "capabilityRow skillRow capabilityRowWithSwitch"}
                key={item.id}
                role="listitem"
              >
                <button aria-label={`Skill ${item.name}`} className="skillRowSelect" onClick={() => onSelect(item.id)} type="button">
                  <span className="capabilityRowMain">
                    <strong>{item.name}</strong>
                    <small>{item.description || readinessLabel(item.readiness)}</small>
                    {metadata && <span className="skillRowMetadata">{metadata}</span>}
                  </span>
                </button>
                <Switch
                  ariaLabel={item.enabled ? `Disable ${item.name}` : `Enable ${item.name}`}
                  checked={item.enabled}
                  className="capabilityRowSwitch"
                  disabled={busy}
                  label={item.enabled ? "Enabled" : "Disabled"}
                  onCheckedChange={(enabled) => void mutate(() => client.request("skill/setEnabled", { name: item.name, enabled, target: mutableTargetForSkill(item) ?? skillInstall.target, scope }))}
                  showLabel={false}
                  size="compact"
                />
              </div>
            );
          })}
        </div>

        <aside className="capabilityDetail skillDetail" aria-label="Skills detail">
          {row ? (
            <>
              <div className="capabilityDetailHeader">
                <div>
                  <h3>{row.name}</h3>
                  <span>{skillDetailSummary(row)}</span>
                </div>
              </div>

              <div className="capabilityActionGrid">
                <button
                  disabled={busy || !mutableTargetForSkill(row)}
                  onClick={() => {
                    const target = mutableTargetForSkill(row);
                    if (!target || !confirmAction(`Uninstall skill ${row.name}?`)) return;
                    void mutate(() => client.request("skill/uninstall", { name: row.name, path: row.location || null, target, scope }));
                  }}
                  title={mutableTargetForSkill(row) ? "Uninstall" : "Only profile/project-installed skills can be uninstalled here"}
                  type="button"
                >
                  <Trash2 size={14} /> Uninstall
                </button>
              </div>

              <SkillDetailFields row={row} detail={detail?.id === row.id ? detail : null} onCopyText={onCopyText} />
            </>
          ) : (
            <div className="capabilityEmpty">Select an item</div>
          )}
        </aside>
      </div>

    </>
  );
}

function SkillDetailFields({
  detail,
  onCopyText,
  row
}: {
  detail: { loading: boolean; value: JsonObject | null; error: string | null } | null;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  row: SkillRow;
}) {
  const value = detail?.value ?? {};
  const linkedFiles = objectField(value, "linked_files");
  const preview = stringField(value, "preview_content") || stringField(value, "content");
  const entrypoint = shouldShowSkillEntrypoint(row) ? row.location : "";
  const detailFields: Array<[string, string]> = [
    ["Entrypoint", entrypoint],
    ["Skill Dir", row.skillDir],
    ["Platform", row.supported ? "supported" : "unsupported"],
    ["Tags", row.tags.join(", ")],
    ["Missing Env", row.missingEnvVars.join(", ")],
    ["Missing Credentials", row.missingCredentialFiles.join(", ")],
    ["Tools", row.requiredTools.join(", ")],
    ["Toolsets", row.requiredToolsets.join(", ")],
    ["Linked Files", linkedFilesText(linkedFiles)]
  ];
  const detailRows = detailFields.filter(([, fieldValue]) => fieldValue.trim().length > 0);

  return (
    <div className="skillDetailBody">
      <div className="skillDetailSummary">
        <dl className="capabilityKeyValues skillKeyValues">
          {detailRows.map(([label, fieldValue]) => <div key={label}><dt>{label}</dt><dd>{fieldValue}</dd></div>)}
        </dl>
        {row.issues.length > 0 && (
          <section className="skillDiagnostics" aria-label="Skill diagnostics">
            {row.issues.map((issue) => <span className="skillIssue" key={issue}>{issue}</span>)}
          </section>
        )}
      </div>
      <section className="skillPreview" aria-label="SKILL.md preview">
        {detail?.loading ? "Loading" : detail?.error ? detail.error : preview ? (
          <MarkdownText
            copyLabel="Copy SKILL.md"
            copyText={preview}
            onCopyText={onCopyText}
            text={boundedText(preview, 4000)}
          />
        ) : "No preview"}
      </section>
    </div>
  );
}

function shouldShowSkillEntrypoint(row: SkillRow): boolean {
  if (!row.location) return false;
  if (!row.skillDir) return true;
  return normalizePathText(row.location) !== `${normalizePathText(row.skillDir)}/SKILL.md`;
}

function normalizePathText(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+$/, "");
}

function CapabilityActions({
  busy,
  client,
  mutate,
  onOAuthSession,
  row,
  scope,
  setToolPolicyDraft,
  tab,
  toolPolicyDraft
}: {
  busy: boolean;
  client: GatewayClient | null;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onOAuthSession(sessionId: string | null): void;
  row: CapabilityRow;
  scope: GatewayRequestScope | null;
  setToolPolicyDraft(value: { enabledTools: string; disabledTools: string }): void;
  tab: CapabilityTab;
  toolPolicyDraft: { enabledTools: string; disabledTools: string };
}) {
  if (!client || !scope) return null;
  if (tab === "tools") {
    const canToggleModes = toolsetModeMutable(row);
    const canRemove = toolsetRemovable(row);
    if (!canToggleModes && !canRemove) return null;
    return (
      <div className="capabilityActionGrid">
        {canToggleModes && (
          <>
            <button disabled={busy} onClick={() => void mutate(() => client.request("tool/setEnabled", { name: row.name, mode: "default", enabled: !modeEnabled(row.raw, "default"), scope }))} type="button">
              Default {modeEnabled(row.raw, "default") ? "Off" : "On"}
            </button>
            <button disabled={busy} onClick={() => void mutate(() => client.request("tool/setEnabled", { name: row.name, mode: "plan", enabled: !modeEnabled(row.raw, "plan"), scope }))} type="button">
              Plan {modeEnabled(row.raw, "plan") ? "Off" : "On"}
            </button>
          </>
        )}
        {canRemove && (
          <button disabled={busy} onClick={() => confirmAction(`Remove toolset ${row.name}?`) && void mutate(() => client.request("tool/remove", { name: row.name, scope }))} type="button">
            <Trash2 size={14} /> Remove
          </button>
        )}
      </div>
    );
  }
  if (tab === "mcp") {
    const policy = objectField(row.raw, "policy");
    const enabledTools = arrayStrings(policy.enabledTools).join(", ");
    const disabledTools = arrayStrings(policy.disabledTools).join(", ");
    return (
      <div className="capabilityStack">
        <div className="capabilityActionGrid">
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/test", { name: row.name, scope }))} type="button">
            <Play size={14} /> Test
          </button>
          <button disabled={busy} onClick={() => void startOAuth(client, scope, row.name, onOAuthSession, mutate)} type="button">
            <LogIn size={14} /> Login
          </button>
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/oauth/logout", { name: row.name, scope }))} type="button">
            <LogOut size={14} /> Logout
          </button>
          <button disabled={busy} onClick={() => confirmAction(`Remove MCP server ${row.name}?`) && void mutate(() => client.request("mcp/remove", { name: row.name, scope }))} type="button">
            <Trash2 size={14} /> Remove
          </button>
        </div>
        <div className="capabilityInlineFields">
          <input aria-label="Enabled MCP tools" onChange={(event) => setToolPolicyDraft({ ...toolPolicyDraft, enabledTools: event.target.value })} placeholder={enabledTools || "enabled tools"} value={toolPolicyDraft.enabledTools} />
          <input aria-label="Disabled MCP tools" onChange={(event) => setToolPolicyDraft({ ...toolPolicyDraft, disabledTools: event.target.value })} placeholder={disabledTools || "disabled tools"} value={toolPolicyDraft.disabledTools} />
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/setToolPolicy", { name: row.name, enabledTools: splitList(toolPolicyDraft.enabledTools || enabledTools) || null, disabledTools: splitList(toolPolicyDraft.disabledTools || disabledTools) ?? [], scope }))} type="button">
            Save Policy
          </button>
        </div>
      </div>
    );
  }
  if (tab === "plugins") {
    const trust = objectField(row.raw, "trust");
    const needsTrust = boolField(trust, "required") && stringField(trust, "status") !== "trusted";
    return (
      <div className="capabilityActionGrid">
        <button disabled={busy} onClick={() => void mutate(() => client.request("plugin/doctor", { selector: row.id, scope }))} type="button">
          <Play size={14} /> Doctor
        </button>
        {needsTrust && (
          <button disabled={busy} onClick={() => confirmAction(`Trust plugin ${row.name} for its current package fingerprint?`) && void mutate(() => client.request("plugin/setTrust", { selector: row.id, trusted: true, scope }))} type="button">
            <LogIn size={14} /> Trust
          </button>
        )}
        <button disabled={busy} onClick={() => confirmAction(`Uninstall plugin ${row.name}?`) && void mutate(() => client.request("plugin/uninstall", { selector: row.id, scope }))} type="button">
          <Trash2 size={14} /> Uninstall
        </button>
      </div>
    );
  }
  return (
    <div className="capabilityActionGrid">
      <button disabled={busy} onClick={() => confirmAction(`Uninstall skill ${row.name}?`) && void mutate(() => client.request("skill/uninstall", { name: row.name, scope }))} type="button">
        <Trash2 size={14} /> Uninstall
      </button>
    </div>
  );
}

function CapabilityForms(props: {
  busy: boolean;
  client: GatewayClient | null;
  mcpDraft: { name: string; transport: string; command: string; url: string; bearerTokenEnvVar: string; oauthClientId: string };
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onClose(): void;
  open: boolean;
  pluginInstall: PluginInstallDraft;
  scope: GatewayRequestScope | null;
  setMcpDraft(value: { name: string; transport: string; command: string; url: string; bearerTokenEnvVar: string; oauthClientId: string }): void;
  setPluginInstall(value: PluginInstallDraft): void;
  setToolDraft(value: { name: string; description: string; tools: string; includes: string; force: boolean }): void;
  tab: CapabilityTab;
  toolDraft: { name: string; description: string; tools: string; includes: string; force: boolean };
}) {
  const { busy, client, mutate, onClose, open, scope, tab } = props;
  if (!client || !scope || !open) return null;
  if (tab === "plugins") {
    const draft = props.pluginInstall;
    const inspectParams = {
      source: draft.source,
      sourceKind: draft.kind,
      gitRef: null,
      npmVersion: draft.kind === "npm" ? draft.npmVersion || null : null,
      npmRegistry: draft.kind === "npm" ? draft.npmRegistry || null : null,
      adapterMode: draft.adapterMode,
      scope
    };
    return (
      <CreatePanel className="capabilityCreatePanel" description="Inspect and install a plugin package." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Install plugin">
        <form className="capabilityForm" onSubmit={(event) => {
          event.preventDefault();
          if (draft.force && !confirmAction("Install plugin with force?")) return;
          void mutate(() => client.request("plugin/install", { ...inspectParams, force: draft.force })).then((ok) => {
            if (ok) onClose();
          });
        }}>
          <input aria-label="Plugin source" onChange={(event) => props.setPluginInstall({ ...draft, source: event.target.value, inspection: null })} placeholder="path, git source, or npm package" value={draft.source} />
          <select aria-label="Plugin source kind" onChange={(event) => props.setPluginInstall({ ...draft, kind: pluginKindValue(event.target.value), inspection: null })} value={draft.kind}>
            <option value="local">Local</option>
            <option value="git">Git</option>
            <option value="npm">npm</option>
          </select>
          {draft.kind === "npm" && (
            <>
              <input aria-label="Npm package version" onChange={(event) => props.setPluginInstall({ ...draft, npmVersion: event.target.value, inspection: null })} placeholder="version" value={draft.npmVersion} />
              <input aria-label="Npm registry" onChange={(event) => props.setPluginInstall({ ...draft, npmRegistry: event.target.value, inspection: null })} placeholder="registry" value={draft.npmRegistry} />
            </>
          )}
          <select aria-label="Plugin adapter mode" onChange={(event) => props.setPluginInstall({ ...draft, adapterMode: pluginAdapterModeValue(event.target.value), inspection: null })} value={draft.adapterMode}>
            <option value="manifest_only">Manifest only</option>
            <option value="adapter_host">Adapter host</option>
            <option value="disabled">Disabled</option>
          </select>
          <label><input checked={draft.force} onChange={(event) => props.setPluginInstall({ ...draft, force: event.target.checked })} type="checkbox" /> Force</label>
          <div className="capabilityFormActions">
            <ActionButton disabled={busy || !draft.source.trim()} icon={<Search size={14} />} onClick={() => {
              void mutate(async () => {
                const result = await client.request("plugin/import/inspect", inspectParams);
                props.setPluginInstall({ ...draft, inspection: objectField(result, "inspection") });
                return result;
              }, { notice: "Inspection complete.", refresh: false });
            }} type="button" variant="neutral">Inspect</ActionButton>
            <ActionButton disabled={busy || !draft.source.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Install</ActionButton>
          </div>
          {draft.inspection && <PluginInspectionSummary inspection={draft.inspection} />}
        </form>
      </CreatePanel>
    );
  }
  if (tab === "mcp") {
    const draft = props.mcpDraft;
    return (
      <CreatePanel className="capabilityCreatePanel" description="Add a profile-scoped stdio or HTTP MCP server." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Add MCP server">
        <form className="capabilityForm" onSubmit={(event) => {
          event.preventDefault();
          void mutate(() => client.request("mcp/upsert", {
            name: draft.name,
            transport: draft.transport,
            command: draft.transport === "stdio" ? draft.command : null,
            url: draft.transport === "streamable_http" ? draft.url : null,
            bearerTokenEnvVar: draft.bearerTokenEnvVar || null,
            oauthClientId: draft.oauthClientId || null,
            scope
          })).then((ok) => {
            if (ok) onClose();
          });
        }}>
          <input aria-label="MCP name" onChange={(event) => props.setMcpDraft({ ...draft, name: event.target.value })} placeholder="name" value={draft.name} />
          <select aria-label="MCP transport" onChange={(event) => props.setMcpDraft({ ...draft, transport: event.target.value })} value={draft.transport}>
            <option value="stdio">stdio</option>
            <option value="streamable_http">HTTP</option>
          </select>
          <input aria-label="MCP command or URL" onChange={(event) => draft.transport === "stdio" ? props.setMcpDraft({ ...draft, command: event.target.value }) : props.setMcpDraft({ ...draft, url: event.target.value })} placeholder={draft.transport === "stdio" ? "command" : "url"} value={draft.transport === "stdio" ? draft.command : draft.url} />
          <input aria-label="Bearer token env var" onChange={(event) => props.setMcpDraft({ ...draft, bearerTokenEnvVar: event.target.value })} placeholder="bearer env" value={draft.bearerTokenEnvVar} />
          <input aria-label="OAuth client id" onChange={(event) => props.setMcpDraft({ ...draft, oauthClientId: event.target.value })} placeholder="client id" value={draft.oauthClientId} />
          <ActionButton disabled={busy || !draft.name.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Save</ActionButton>
        </form>
      </CreatePanel>
    );
  }
  const draft = props.toolDraft;
  const builtInToolsetName = isBuiltInToolsetName(draft.name);
  return (
    <CreatePanel className="capabilityCreatePanel" description="Create or overwrite a custom toolset." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Create toolset">
      <form className="capabilityForm" onSubmit={(event) => {
        event.preventDefault();
        if (isBuiltInToolsetName(draft.name)) return;
        if (draft.force && !confirmAction("Overwrite toolset?")) return;
        void mutate(() => client.request("tool/create", {
          name: draft.name,
          description: draft.description || null,
          tools: splitList(draft.tools) ?? [],
          includes: splitList(draft.includes) ?? [],
          force: draft.force,
          scope
        })).then((ok) => {
          if (ok) onClose();
        });
      }}>
        <input aria-label="Toolset name" onChange={(event) => props.setToolDraft({ ...draft, name: event.target.value })} placeholder="name" value={draft.name} />
        <input aria-label="Toolset description" onChange={(event) => props.setToolDraft({ ...draft, description: event.target.value })} placeholder="description" value={draft.description} />
        <input aria-label="Tool names" onChange={(event) => props.setToolDraft({ ...draft, tools: event.target.value })} placeholder="tools" value={draft.tools} />
        <input aria-label="Included toolsets" onChange={(event) => props.setToolDraft({ ...draft, includes: event.target.value })} placeholder="includes" value={draft.includes} />
        <label><input checked={draft.force} onChange={(event) => props.setToolDraft({ ...draft, force: event.target.checked })} type="checkbox" /> Force</label>
        <ActionButton disabled={busy || !draft.name.trim() || builtInToolsetName} icon={<Plus size={14} />} title={builtInToolsetName ? "Built-in toolsets cannot be overwritten" : "Save"} type="submit" variant="primary">Save</ActionButton>
      </form>
    </CreatePanel>
  );
}

async function requestTab(client: GatewayClient, tab: CapabilityTab, scope: GatewayRequestScope) {
  if (tab === "skills") return client.request("skill/list", { scope });
  if (tab === "plugins") return client.request("plugin/list", { scope });
  if (tab === "mcp") return client.request("mcp/list", { scope });
  return client.request("tool/list", { scope });
}

async function setCapabilityEnabled(client: GatewayClient | null, scope: GatewayRequestScope | null, tab: CapabilityTab, row: CapabilityRow, enabled: boolean) {
  if (!client || !scope) return;
  if (tab === "skills") return client.request("skill/setEnabled", { name: row.name, enabled, scope });
  if (tab === "plugins") return client.request("plugin/setEnabled", { selector: row.id, enabled, scope });
  if (tab === "mcp") return client.request("mcp/setEnabled", { name: row.name, enabled, scope });
}

async function startOAuth(
  client: GatewayClient,
  scope: GatewayRequestScope,
  name: string,
  onOAuthSession: (sessionId: string | null) => void,
  mutate: (action: () => Promise<unknown>) => Promise<boolean>
) {
  await mutate(async () => {
    const result = await client.request("mcp/oauth/start", { name, scope });
    const url = stringField(result, "authorizationUrl");
    const sessionId = stringField(result, "sessionId");
    if (url) window.open(url, "_blank", "noopener,noreferrer");
    if (sessionId) onOAuthSession(sessionId);
    return result;
  });
}

function skillRowsFromData(data: JsonObject | null): SkillRow[] {
  if (!data) return [];
  return arrayObjects(data.skills).map((skill) => {
    const location = stringField(skill, "location");
    const id = stringField(skill, "id") || location || stringField(skill, "name");
    const readiness = stringField(skill, "readiness_status") || "available";
    const source = stringField(skill, "source") || "unknown";
    const sourceLabel = skillSourceDisplayLabel(stringField(skill, "source_label") || source);
    return {
      id,
      name: stringField(skill, "name"),
      description: stringField(skill, "description"),
      enabled: boolField(skill, "enabled"),
      status: readiness,
      badges: [sourceLabel, readiness].filter(Boolean),
      raw: skill,
      collisionGroup: arrayStrings(skill.collision_group),
      issues: arrayStrings(skill.issues),
      location,
      missingCredentialFiles: arrayStrings(skill.missing_credential_files),
      missingEnvVars: arrayStrings(skill.missing_required_environment_variables),
      promptVisible: boolField(skill, "prompt_visible"),
      readiness,
      requiredTools: arrayStrings(skill.required_tools),
      requiredToolsets: arrayStrings(skill.required_toolsets),
      skillDir: stringField(skill, "skill_dir"),
      source,
      sourceLabel,
      supported: skill.supported_on_current_platform !== false,
      tags: arrayStrings(skill.tags)
    };
  });
}

function readinessLabel(value: string): string {
  if (value === "setup_needed") return "Setup Needed";
  if (value === "unsupported") return "Unsupported";
  if (value === "available" || value === "ready") return "Available";
  return labelForKey(value || "unknown");
}

function skillSourceDisplayLabel(value: string): string {
  switch (value.trim()) {
    case "project":
    case "agents":
    case "Project":
      return "Project";
    case "explicit":
    case "global":
    case "agents_global":
    case "config":
    case "install_source":
    case "User":
      return "User";
    case "plugin":
    case "system":
    case "builtin":
    case "built_in":
    case "core":
    case "System":
      return "System";
    default:
      return "";
  }
}

function skillRowMetadata(row: SkillRow): string {
  const values = [row.sourceLabel];
  if (!row.supported) {
    values.push("Unsupported");
  } else if (row.readiness !== "available" && row.readiness !== "ready") {
    values.push(readinessLabel(row.readiness));
  }
  if (row.collisionGroup.length > 0) values.push("Collision");
  return values.filter(Boolean).join(" · ");
}

function skillDetailSummary(row: SkillRow): string {
  return [row.sourceLabel, readinessLabel(row.readiness)].filter(Boolean).join(" · ");
}

function mutableTargetForSkill(row: SkillRow): "global" | "project" | null {
  if (row.source === "project") return "project";
  if (row.source === "global") return "global";
  return null;
}

function linkedFilesText(value: JsonObject): string {
  return Object.entries(value)
    .flatMap(([group, files]) => arrayStrings(files).map((file) => file.startsWith(`${group}/`) ? file : `${group}/${file}`))
    .join(", ");
}

function boundedText(value: string, limit: number): string {
  return value.length > limit ? `${value.slice(0, limit)}...[truncated]` : value;
}

function rowsForTab(tab: CapabilityTab, data: JsonObject | null): CapabilityRow[] {
  if (!data) return [];
  if (tab === "skills") {
    return arrayObjects(data.skills).map((skill) => ({
      id: stringField(skill, "id") || stringField(skill, "location") || stringField(skill, "name"),
      name: stringField(skill, "name"),
      description: stringField(skill, "description"),
      enabled: boolField(skill, "enabled"),
      status: stringField(skill, "readiness_status") || "ready",
      badges: [
        skillSourceDisplayLabel(stringField(skill, "source_label") || stringField(skill, "source")),
        stringField(skill, "category")
      ].filter(Boolean),
      raw: skill
    }));
  }
  if (tab === "plugins") {
    return arrayObjects(data.plugins).map((plugin) => ({
      id: stringField(plugin, "source_id") || stringField(plugin, "name"),
      name: stringField(plugin, "name"),
      description: stringField(plugin, "description"),
      enabled: boolField(plugin, "enabled"),
      status: stringField(plugin, "status") || stringField(plugin, "readiness") || "Installed",
      badges: [stringField(plugin, "manifest_kind"), stringField(plugin, "source_kind"), stringField(plugin, "source")].filter(Boolean),
      raw: plugin
    }));
  }
  if (tab === "mcp") {
    return arrayObjects(data.servers).map((server) => {
      const transport = objectField(server, "transport");
      return {
        id: stringField(server, "name"),
        name: stringField(server, "name"),
        description: stringField(transport, "url") || stringField(transport, "command"),
        enabled: boolField(server, "enabled"),
        status: stringField(transport, "kind") || "mcp",
        badges: [stringField(server, "sourceKind"), boolField(server, "required") ? "required" : ""].filter(Boolean),
        raw: server
      };
    });
  }
  const modes = objectField(data, "modes");
  return arrayObjects(data.toolsets).map((toolset) => ({
    id: stringField(toolset, "name"),
    name: stringField(toolset, "name"),
    description: stringField(toolset, "description") || arrayStrings(toolset.tools).join(", "),
    enabled: modeEnabled({ ...toolset, modes }, "default"),
    status: stringField(toolset, "source") || "toolset",
    badges: [`default:${modeEnabled({ ...toolset, modes }, "default") ? "on" : "off"}`, `plan:${modeEnabled({ ...toolset, modes }, "plan") ? "on" : "off"}`],
    raw: { ...toolset, modes }
  }));
}

function modeEnabled(row: JsonObject, mode: "default" | "plan"): boolean {
  const modes = objectField(row, "modes");
  const modeConfig = objectField(modes, mode);
  const name = stringField(row, "name");
  if (arrayStrings(modeConfig.disabled_toolsets).includes(name)) return false;
  const enabled = arrayStrings(modeConfig.enabled_toolsets);
  return enabled.length === 0 || enabled.includes(name);
}

function toolsetModeMutable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).mode_mutable;
  return typeof value === "boolean" ? value : true;
}

function toolsetRemovable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).removable;
  return typeof value === "boolean" ? value : stringField(row.raw, "source") === "custom";
}

function isBuiltInToolsetName(value: string): boolean {
  return ["coding-core", "web"].includes(value.trim());
}

function PluginInspectionSummary({ inspection }: { inspection: JsonObject }) {
  const lanes = arrayStrings(inspection.target_lanes);
  const unsupported = arrayStrings(inspection.unsupported_lanes);
  const stages = arrayObjects(inspection.stages);
  return (
    <section className="pluginInspection" aria-label="Plugin inspection">
      <div>
        <strong>{stringField(inspection, "name") || "Plugin"}</strong>
        <span>{[stringField(inspection, "framework"), stringField(inspection, "status")].filter(Boolean).join(" / ")}</span>
      </div>
      <dl className="capabilityKeyValues pluginInspectionGrid">
        <div><dt>Source</dt><dd>{stringField(inspection, "source_kind") || "local"}</dd></div>
        <div><dt>Mode</dt><dd>{stringField(inspection, "adapter_mode") || "manifest_only"}</dd></div>
        {lanes.length > 0 && <div><dt>Lanes</dt><dd>{lanes.join(", ")}</dd></div>}
        {unsupported.length > 0 && <div><dt>Unsupported</dt><dd>{unsupported.join(", ")}</dd></div>}
      </dl>
      {stages.length > 0 && (
        <div className="pluginInspectionStages">
          {stages.slice(0, 5).map((stage, index) => (
            <span className="capabilityChip" key={`${stringField(stage, "stage")}-${index}`}>
              {stringField(stage, "stage")}: {stringField(stage, "status")}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function pluginKindValue(value: string): PluginInstallDraft["kind"] {
  return value === "git" || value === "npm" ? value : "local";
}

function pluginAdapterModeValue(value: string): PluginInstallDraft["adapterMode"] {
  if (value === "adapter_host" || value === "disabled") return value;
  return "manifest_only";
}

function KeyValueView({ value }: { value: JsonObject }) {
  const entries = Object.entries(value).filter(([, entry]) => entry !== null && entry !== undefined);
  return (
    <dl className="capabilityKeyValues">
      {entries.slice(0, 14).map(([key, entry]) => (
        <div key={key}>
          <dt>{labelForKey(key)}</dt>
          <dd>{displayValue(entry)}</dd>
        </div>
      ))}
    </dl>
  );
}

function objectValue(value: unknown): JsonObject {
  return value && typeof value === "object" && !Array.isArray(value) ? value as JsonObject : {};
}

function objectField(value: unknown, key: string): JsonObject {
  const object = objectValue(value);
  return objectValue(object[key]);
}

function arrayObjects(value: unknown): JsonObject[] {
  return Array.isArray(value) ? value.map(objectValue) : [];
}

function arrayStrings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
}

function stringField(value: unknown, key: string): string {
  const entry = objectValue(value)[key];
  return typeof entry === "string" ? entry : "";
}

function boolField(value: unknown, key: string): boolean {
  return objectValue(value)[key] === true;
}

function displayValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.map(displayValue).join(", ");
  if (value && typeof value === "object") return JSON.stringify(value);
  return "";
}

function splitList(value: string): string[] | null {
  const values = value.split(",").map((entry) => entry.trim()).filter(Boolean);
  return values.length ? values : null;
}

function tabLabel(tab: CapabilityTab): string {
  return TABS.find((item) => item.id === tab)?.label ?? tab;
}

function createActionLabel(tab: CapabilityTab): string {
  if (tab === "skills") return "Install skill";
  if (tab === "plugins") return "Install plugin";
  if (tab === "mcp") return "Add MCP server";
  return "Create toolset";
}

function hasCapabilityRowSwitch(tab: CapabilityTab): boolean {
  return tab === "plugins" || tab === "mcp";
}

function rowKindLabel(tab: CapabilityTab): string {
  if (tab === "plugins") return "Plugin";
  if (tab === "mcp") return "MCP";
  if (tab === "tools") return "Toolset";
  return "Skill";
}

function CapabilityBadges({ row }: { row: CapabilityRow }) {
  if (row.badges.length === 0) return null;
  return (
    <span className="capabilityRowMeta">
      {row.badges.slice(0, 2).map((badge) => <span className="capabilityChip" key={badge}>{badge}</span>)}
    </span>
  );
}

function labelForKey(value: string): string {
  return value.replace(/_/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function confirmAction(message: string): boolean {
  return window.confirm(message);
}
