import { useMemo, useState } from "react";
import { ArrowLeft, ArrowRight, ExternalLink, Globe2, MessageSquarePlus, RefreshCw } from "lucide-react";
import type { RightWorkspaceBrowserState } from "../types";

export function BrowserPanel({
  hostKind,
  onOpenExternal,
  onStateChange,
  state,
  sessionId
}: {
  hostKind: string;
  onOpenExternal(url: string): void | Promise<void>;
  onStateChange(state: RightWorkspaceBrowserState): void;
  state: RightWorkspaceBrowserState;
  sessionId: string | null;
}) {
  const [error, setError] = useState<string | null>(null);
  const { address, currentUrl, reloadKey } = state;
  const desktopHostAvailable = false;
  const previewOnly = hostKind !== "desktop" || !desktopHostAvailable;
  const pageTitle = useMemo(() => browserPageTitle(currentUrl), [currentUrl]);

  function navigate(value: string) {
    const normalized = normalizeBrowserUrl(value);
    if (!normalized.ok) {
      setError(normalized.error);
      return;
    }
    setError(null);
    onStateChange({
      address: normalized.url,
      currentUrl: normalized.url,
      reloadKey: reloadKey + 1
    });
  }

  return (
    <section className="browserPanel" aria-label="Browser">
      <div className="browserToolbar" aria-label="Browser controls">
        <button aria-label="Back" disabled title="Back" type="button">
          <ArrowLeft size={14} />
        </button>
        <button aria-label="Forward" disabled title="Forward" type="button">
          <ArrowRight size={14} />
        </button>
        <button
          aria-label="Reload"
          disabled={!currentUrl}
          onClick={() => onStateChange({ ...state, reloadKey: reloadKey + 1 })}
          title="Reload"
          type="button"
        >
          <RefreshCw size={14} />
        </button>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            navigate(address);
          }}
        >
          <input
            aria-label="Browser address"
            onChange={(event) => onStateChange({ ...state, address: event.currentTarget.value })}
            placeholder="https://example.com"
            value={address}
          />
        </form>
        <button
          aria-label="Annotate page"
          disabled
          title={previewOnly ? "Desktop required" : "Annotate page"}
          type="button"
        >
          <MessageSquarePlus size={14} />
        </button>
        <button
          aria-label="Open externally"
          disabled={!currentUrl}
          onClick={() => {
            if (currentUrl) void onOpenExternal(currentUrl);
          }}
          title="Open externally"
          type="button"
        >
          <ExternalLink size={14} />
        </button>
      </div>
      <div className="browserCanvas">
        {!currentUrl ? (
          <div className="browserEmpty">
            <Globe2 size={28} aria-hidden />
            <h2>Browser</h2>
            <form
              onSubmit={(event) => {
                event.preventDefault();
                navigate(address);
              }}
            >
              <input
                aria-label="Open URL"
                onChange={(event) => onStateChange({ ...state, address: event.currentTarget.value })}
                placeholder="https://example.com"
                value={address}
              />
              <button type="submit">Open</button>
            </form>
            {sessionId && <small title={sessionId}>{sessionId}</small>}
          </div>
        ) : desktopHostAvailable ? (
          <div className="browserNativeHost" aria-label={pageTitle}>
            <span>Browser host</span>
          </div>
        ) : (
          <iframe
            key={`${currentUrl}:${reloadKey}`}
            src={currentUrl}
            title={pageTitle}
          />
        )}
        {error && <div className="browserError" role="alert">{error}</div>}
        {currentUrl && previewOnly && (
          <div className="browserModeBadge">Preview only</div>
        )}
      </div>
    </section>
  );
}

type NormalizeBrowserUrlResult =
  | { ok: true; url: string }
  | { error: string; ok: false };

export function normalizeBrowserUrl(input: string): NormalizeBrowserUrlResult {
  const trimmed = input.trim();
  if (!trimmed) {
    return { error: "Enter a URL.", ok: false };
  }
  const scheme = /^([A-Za-z][A-Za-z0-9+.-]*):/.exec(trimmed);
  const hostPortShorthand = /^(?:\[[^\]]+\]|[^/?#:]+):\d+(?:[/?#]|$)/.test(trimmed);
  let candidate = trimmed;
  if (!scheme || hostPortShorthand) {
    let probe: URL;
    try {
      probe = new URL(`http://${trimmed}`);
    } catch {
      return { error: "Enter a valid URL.", ok: false };
    }
    candidate = `${isLoopbackHost(probe.hostname) ? "http" : "https"}://${trimmed}`;
  }
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return { error: "Enter a valid URL.", ok: false };
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return { error: "Browser supports http and https URLs.", ok: false };
  }
  return { ok: true, url: url.toString() };
}

function isLoopbackHost(hostname: string): boolean {
  const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, "");
  return normalized === "localhost"
    || normalized.endsWith(".localhost")
    || normalized.startsWith("127.")
    || normalized === "::1";
}

function browserPageTitle(url: string | null): string {
  if (!url) {
    return "Browser";
  }
  try {
    return new URL(url).hostname || url;
  } catch {
    return url;
  }
}
