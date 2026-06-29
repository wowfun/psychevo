import { diffDisplayPath, diffLineStats, type ParsedDiffFile } from "../diff";
import type { EvidenceDisplay, ToolDetailSection } from "../toolEvidence";

export function ToolDetail({ display }: { display: EvidenceDisplay }) {
  return (
    <div className="pevo-toolDetail">
      {display.sections.map((section, index) => <ToolDetailSectionView key={`${section.title}:${index}`} section={section} />)}
    </div>
  );
}

function ToolDetailSectionView({ section }: { section: ToolDetailSection }) {
  const toneClass = section.tone && section.tone !== "default" ? ` is-${section.tone}` : "";
  if (section.kind === "diff") {
    return (
      <section className={`pevo-toolSection is-diff${toneClass}`}>
        <InlineDiff files={section.files} />
      </section>
    );
  }
  if (section.kind === "kv") {
    return (
      <section className={`pevo-toolSection is-kv${toneClass}`}>
        <h4>{section.title}</h4>
        <dl>
          {section.rows.map((row) => (
            <div key={`${row.label}:${row.value}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    );
  }
  return (
    <section className={`pevo-toolSection is-text${section.code ? " is-code" : ""}${toneClass}`}>
      {section.title.trim() ? <h4>{section.title}</h4> : null}
      <pre>{section.text}</pre>
    </section>
  );
}

function InlineDiff({ files }: { files: ParsedDiffFile[] }) {
  return (
    <div className="pevo-inlineDiff" aria-label="Inline diff">
      {files.map((file, fileIndex) => {
        const stats = diffLineStats(file);
        const metaRows = inlineDiffMetaRows(file);
        return (
          <article className="pevo-inlineDiffFile" key={`${file.path}:${fileIndex}`}>
            <header>
              <span className="pevo-inlineDiffPath" title={diffDisplayPath(file)}>
                {diffDisplayPath(file)}
              </span>
              <span className="pevo-inlineDiffStats" aria-label={`${stats.additions} additions, ${stats.deletions} deletions`}>
                <span className="pevo-inlineDiffAdd">+{stats.additions}</span>
                <span className="pevo-inlineDiffDelete">-{stats.deletions}</span>
              </span>
            </header>
            {metaRows.length > 0 && (
              <div className="pevo-inlineDiffMetaRows">
                {metaRows.map((row, index) => <div key={`${row}:${index}`}>{row}</div>)}
              </div>
            )}
            {file.hunks.length === 0 ? (
              <p className="pevo-inlineDiffEmpty">No line diff available.</p>
            ) : (
              file.hunks.map((hunk, hunkIndex) => (
                <section className="pevo-inlineDiffHunk" key={`${hunk.header}:${hunkIndex}`}>
                  <div className="pevo-inlineDiffHunkHeader">{hunk.header}</div>
                  <div className="pevo-inlineDiffLines">
                    {hunk.lines.map((line, lineIndex) => {
                      const lineNumber = line.newNumber ?? line.oldNumber ?? "";
                      return (
                        <div className={`pevo-inlineDiffLine is-${line.kind}`} key={`${line.oldNumber}:${line.newNumber}:${lineIndex}`}>
                          <span className="pevo-inlineDiffNumber">{lineNumber}</span>
                          <span className="pevo-inlineDiffMarker">{line.marker}</span>
                          <code>{line.text || " "}</code>
                        </div>
                      );
                    })}
                  </div>
                </section>
              ))
            )}
          </article>
        );
      })}
    </div>
  );
}

function inlineDiffMetaRows(file: ParsedDiffFile): string[] {
  return file.headers.flatMap((line) => {
    if (
      line.startsWith("diff --git ") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ")
    ) {
      return [];
    }
    if (line.startsWith("rename from ")) {
      return [`renamed from ${line.slice("rename from ".length)}`];
    }
    if (line.startsWith("rename to ")) {
      return [`renamed to ${line.slice("rename to ".length)}`];
    }
    return line.trim() ? [line] : [];
  });
}
