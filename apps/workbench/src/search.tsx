import { useEffect, useState } from "react";
import { Search } from "lucide-react";
import type { SessionSummary, ThreadSnapshot } from "@psychevo/protocol";
import { asRecord, stringField } from "./data";
import { shortSessionId } from "./session-utils";
import type { SearchResult } from "./types";

export function SearchPage({
  loadThreadSearchText,
  sessions,
  onOpenSession,
  onOpenTranscript
}: {
  loadThreadSearchText(threadId: string): Promise<string>;
  sessions: SessionSummary[];
  onOpenSession(threadId: string): void;
  onOpenTranscript(): void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);

  useEffect(() => {
    const needle = normalizeSearchText(query);
    if (!needle) {
      setResults([]);
      setSearching(false);
      return;
    }
    let cancelled = false;
    setSearching(true);
    const timer = window.setTimeout(() => {
      void (async () => {
        const next: SearchResult[] = [];
        const seen = new Set<string>();
        for (const session of sessions) {
          const title = session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id);
          const workspace = session.project?.label ?? "";
          const summaryHaystack = normalizeSearchText(`${session.id} ${title} ${session.preview ?? ""} ${workspace} ${session.workdir}`);
          if (summaryHaystack.includes(needle)) {
            next.push({
              excerpt: session.id,
              id: session.id,
              kind: "session",
              subtitle: `${workspace || "workspace"} · ${session.visibleEntryCount ?? session.messageCount ?? 0} entries`,
              title
            });
            seen.add(`${session.id}:session`);
          }
        }
        for (const session of sessions.filter((item) => (item.messageCount ?? 0) > 0)) {
          const text = await loadThreadSearchText(session.id);
          const normalized = normalizeSearchText(text);
          if (cancelled) {
            return;
          }
          if (normalized.includes(needle)) {
            const key = `${session.id}:message`;
            if (!seen.has(key)) {
              next.push({
                excerpt: searchExcerpt(text, query),
                id: session.id,
                kind: "message",
                subtitle: session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id),
                title: "Message match"
              });
              seen.add(key);
            }
          }
        }
        if (!cancelled) {
          setResults(next);
          setSearching(false);
        }
      })();
    }, 180);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [loadThreadSearchText, query, sessions]);

  return (
    <section className="centerPage searchPage" aria-label="Search">
      <header>
        <Search size={18} />
        <div>
          <h2>Search</h2>
          <p>Search session ids, session names, and message text.</p>
        </div>
      </header>
      <input autoFocus placeholder="Search current workspace" value={query} onChange={(event) => setQuery(event.target.value)} />
      {query.trim() && results.length > 0 ? (
        <div className="searchResults">
          {results.map((result) => (
            <button key={`${result.id}:${result.kind}`} onClick={() => onOpenSession(result.id)} type="button">
              <strong>{result.title}</strong>
              <span>{result.subtitle}</span>
              <small>{result.excerpt}</small>
            </button>
          ))}
        </div>
      ) : (
        <div className="emptyLedger">
          <span>{query.trim() ? (searching ? "Searching sessions..." : "No matches in this workspace.") : "Type to search local session material."}</span>
          <button onClick={onOpenTranscript} type="button">Back to transcript</button>
        </div>
      )}
    </section>
  );
}

export function transcriptSearchText(entries: ThreadSnapshot["entries"]): string {
  return entries
    .flatMap((entry) => [
      entry.role,
      ...entry.blocks.flatMap((block) => {
        const record = asRecord(block);
        return [
          stringField(record.title),
          stringField(record.body),
          stringField(record.preview),
          stringField(record.detail)
        ];
      })
    ])
    .filter(Boolean)
    .join("\n");
}

function normalizeSearchText(value: string): string {
  return value.trim().toLowerCase();
}

function searchExcerpt(text: string, query: string): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return "Message text matched.";
  }
  const index = normalized.toLowerCase().indexOf(query.trim().toLowerCase());
  if (index < 0) {
    return normalized.slice(0, 160);
  }
  const start = Math.max(0, index - 56);
  const end = Math.min(normalized.length, index + query.trim().length + 96);
  const prefix = start > 0 ? "..." : "";
  const suffix = end < normalized.length ? "..." : "";
  return `${prefix}${normalized.slice(start, end)}${suffix}`;
}
