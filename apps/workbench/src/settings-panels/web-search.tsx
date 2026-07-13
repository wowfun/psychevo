import { useEffect, useMemo, useState, type ReactNode } from "react";
import { scopeForCwd, type GatewayClient } from "@psychevo/client";
import type { WebSearchSettingsView } from "@psychevo/protocol";

const CREDENTIALS = [
  ["EXA_API_KEY", "Exa", "exa"],
  ["PARALLEL_API_KEY", "Parallel", "parallel"],
  ["BRAVE_SEARCH_API_KEY", "Brave", "brave"],
  ["SEARXNG_URL", "SearXNG URL", "searxng"]
] as const;

export function WebSearchSettingsPanel({ client, cwd, disabled }: { client: GatewayClient | null; cwd: string; disabled: boolean }) {
  const [draft, setDraft] = useState<WebSearchSettingsView | null>(null);
  const [credentialValues, setCredentialValues] = useState<Record<string, string>>({});
  const [message, setMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let active = true;
    if (!client) return;
    client.request("web/search/settings/read", { cwd }).then((value) => {
      if (active) setDraft(value);
    }).catch((error: unknown) => active && setMessage(error instanceof Error ? error.message : String(error)));
    return () => { active = false; };
  }, [client, cwd]);
  const storageRequired = draft?.returnTokenBudget === "unlimited";
  const canSave = useMemo(() => Boolean(client && draft && !disabled && !saving && (!storageRequired || draft.backgroundStorageAcknowledged)), [client, draft, disabled, saving, storageRequired]);
  if (!draft) return <p className="settingsEmptyState">{message ?? "Loading web search settings…"}</p>;
  const patch = (next: Partial<WebSearchSettingsView>) => setDraft((current) => current ? { ...current, ...next } : current);
  const save = async () => {
    if (!client || !canSave) return;
    setSaving(true); setMessage(null);
    try {
      const next = await client.request("web/search/settings/update", {
        scope: scopeForCwd(cwd), search: draft, credentialValues
      });
      setDraft(next); setCredentialValues({}); setMessage("Saved to the active profile.");
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setSaving(false); }
  };
  return (
    <div className="settingsRows webSearchSettings">
      <Setting label="Execution"><select disabled={disabled} value={draft.execution} onChange={(event) => patch({ execution: event.currentTarget.value })}><option value="auto">Auto</option><option value="local">Local</option><option value="hosted">Hosted</option></select></Setting>
      <Setting label="Local backend"><select disabled={disabled} value={draft.backend} onChange={(event) => patch({ backend: event.currentTarget.value })}><option value="auto">Auto</option><option value="exa">Exa</option><option value="parallel">Parallel</option><option value="searxng">SearXNG</option><option value="brave">Brave</option></select></Setting>
      <div className="settingsRow"><div><strong>Availability</strong><span>Secrets stay in the profile .env and are returned only as present or missing.</span></div><div className="webSearchCredentialStatus">{Object.entries(draft.credentials).map(([name, status]) => <span key={name}>{name}: {status}</span>)}</div></div>
      {CREDENTIALS.map(([key, label, statusKey]) => <Setting key={key} label={label}><input autoComplete="off" disabled={disabled} placeholder={draft.credentials[statusKey] === "present" ? "Present — replace value" : "Set in profile .env"} type="password" value={credentialValues[key] ?? ""} onChange={(event) => setCredentialValues((values) => ({ ...values, [key]: event.currentTarget.value }))} /></Setting>)}
      <Setting label="External access"><select disabled={disabled} value={draft.externalAccess} onChange={(event) => patch({ externalAccess: event.currentTarget.value })}><option value="live">Live</option><option value="cached">Cached</option></select></Setting>
      <Setting label="Context size"><select disabled={disabled} value={draft.contextSize} onChange={(event) => patch({ contextSize: event.currentTarget.value })}><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option></select></Setting>
      <Setting label="Return budget"><select disabled={disabled} value={draft.returnTokenBudget} onChange={(event) => patch({ returnTokenBudget: event.currentTarget.value })}><option value="default">Default</option><option value="unlimited">Unlimited</option></select></Setting>
      {storageRequired && <label className="settingsRow"><div><strong>Allow temporary provider storage</strong><span>Unlimited search uses OpenAI background mode with store=true.</span></div><input checked={draft.backgroundStorageAcknowledged} disabled={disabled} type="checkbox" onChange={(event) => patch({ backgroundStorageAcknowledged: event.currentTarget.checked })} /></label>}
      <Setting label="Content"><div className="webSearchChecks"><label><input checked={draft.contentTypes.includes("text")} type="checkbox" onChange={(event) => patch({ contentTypes: toggle(draft.contentTypes, "text", event.currentTarget.checked) })} /> Text</label><label><input checked={draft.contentTypes.includes("image")} type="checkbox" onChange={(event) => patch({ contentTypes: toggle(draft.contentTypes, "image", event.currentTarget.checked) })} /> Images</label></div></Setting>
      <Setting label="Allowed domains"><input disabled={disabled} placeholder="example.com, docs.example.com" value={draft.allowedDomains.join(", ")} onChange={(event) => patch({ allowedDomains: csv(event.currentTarget.value) })} /></Setting>
      <Setting label="Blocked domains"><input disabled={disabled} placeholder="example.net" value={draft.blockedDomains.join(", ")} onChange={(event) => patch({ blockedDomains: csv(event.currentTarget.value) })} /></Setting>
      <div className="settingsActions"><button disabled={!canSave} onClick={save} type="button">{saving ? "Saving…" : "Save web search"}</button>{message && <small>{message}</small>}</div>
    </div>
  );
}

function Setting({ label, children }: { label: string; children: ReactNode }) {
  return <label className="settingsRow"><div><strong>{label}</strong></div>{children}</label>;
}
function csv(value: string): string[] { return value.split(",").map((item) => item.trim()).filter(Boolean); }
function toggle(values: string[], value: string, checked: boolean): string[] { return checked ? Array.from(new Set([...values, value])) : values.filter((item) => item !== value); }
