// References & citations for one paper, from OpenAlex (read-only / suggest
// only). Each entry is tagged as already-in-library (clickable) or missing
// (with a DOI link); high-impact missing references are surfaced as "seminal
// works you're missing". Fetched lazily when the tab opens, cached server-side.

import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "../api";
import type { CitationGraph, Item, RelatedPaper } from "../types";
import {
  IconAlert,
  IconCheck,
  IconExternalLink,
  IconLibrary,
  IconLoader,
  IconRefresh,
  IconShare2,
} from "./icons";

interface Props {
  item: Item;
  onOpenItem?: (key: string) => void;
}

/** A missing reference must clear this citation count to be "seminal". */
const SEMINAL_MIN_CITATIONS = 50;
const SEMINAL_MAX = 5;

export default function CitationPanel({ item, onOpenItem }: Props) {
  const [graph, setGraph] = useState<CitationGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = (refresh: boolean) => {
    setLoading(true);
    setError(null);
    api
      .fetchCitationGraph(item.key, refresh)
      .then(setGraph)
      .catch((e) => setError(api.errorMessage(e)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .fetchCitationGraph(item.key, false)
      .then((g) => {
        if (!cancelled) setGraph(g);
      })
      .catch((e) => {
        if (!cancelled) setError(api.errorMessage(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [item.key]);

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center gap-2 text-sm text-muted">
        <IconLoader size={16} /> Looking up references…
      </div>
    );
  }

  if (error) {
    return (
      <Centered>
        <p className="flex items-start gap-1.5 rounded-md bg-danger-soft px-3 py-2 text-xs text-danger">
          <IconAlert size={13} className="mt-0.5 shrink-0" />
          <span>{error}</span>
        </p>
        <button className="btn-secondary mt-3" onClick={() => load(false)}>
          <IconRefresh size={14} /> Retry
        </button>
      </Centered>
    );
  }

  if (!graph || graph.fetchFailed) {
    const noDoi = !item.doi;
    return (
      <Centered>
        <span className="mb-2 flex h-11 w-11 items-center justify-center rounded-full bg-inset text-faint">
          <IconShare2 size={20} />
        </span>
        <p className="text-sm font-medium">No citation data</p>
        <p className="mt-1 max-w-sm text-xs text-muted">
          {noDoi
            ? "This item has no DOI, so its references and citations can't be looked up. Add a DOI in Zotero and refresh."
            : "Couldn't reach the citation source right now — it may be rate-limited. Try again in a moment."}
        </p>
        {!noDoi && (
          <button className="btn-secondary mt-3" onClick={() => load(true)}>
            <IconRefresh size={14} /> Retry
          </button>
        )}
      </Centered>
    );
  }

  const missingSeminal = graph.references
    .filter((r) => !r.inLibraryKey && r.citedByCount >= SEMINAL_MIN_CITATIONS)
    .slice(0, SEMINAL_MAX);
  const inLibRefs = graph.references.filter((r) => r.inLibraryKey).length;

  return (
    <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-6 pt-4">
      <div className="flex items-center gap-2">
        <span className="text-muted">
          Cited by <span className="font-semibold text-text">{graph.citedByCount}</span>
          {" · "}
          {inLibRefs} of {graph.references.length} references in your library
        </span>
        <div className="flex-1" />
        <button
          className="btn-ghost py-0.5! text-xs"
          title="Re-fetch from OpenAlex"
          onClick={() => load(true)}
        >
          <IconRefresh size={12} /> Refresh
        </button>
      </div>

      {missingSeminal.length > 0 && (
        <section className="rounded-lg border border-warn/40 bg-warn-soft p-3">
          <h3 className="mb-2 flex items-center gap-1.5 text-xs font-semibold text-warn">
            <IconAlert size={13} /> Seminal works you're missing
          </h3>
          <ul className="space-y-1.5">
            {missingSeminal.map((p, i) => (
              <PaperRow key={`s${i}`} paper={p} onOpenItem={onOpenItem} />
            ))}
          </ul>
        </section>
      )}

      <Section
        title="References"
        empty="No references found for this paper."
        papers={graph.references}
        onOpenItem={onOpenItem}
      />
      <Section
        title="Cited by"
        empty="No citing papers found (or this paper is very recent)."
        papers={graph.citations}
        onOpenItem={onOpenItem}
      />
    </div>
  );
}

function Section({
  title,
  empty,
  papers,
  onOpenItem,
}: {
  title: string;
  empty: string;
  papers: RelatedPaper[];
  onOpenItem?: (key: string) => void;
}) {
  return (
    <section>
      <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
        {title} ({papers.length})
      </h3>
      {papers.length === 0 ? (
        <p className="text-sm text-muted">{empty}</p>
      ) : (
        <ul className="space-y-1.5">
          {papers.map((p, i) => (
            <PaperRow key={i} paper={p} onOpenItem={onOpenItem} />
          ))}
        </ul>
      )}
    </section>
  );
}

function PaperRow({
  paper,
  onOpenItem,
}: {
  paper: RelatedPaper;
  onOpenItem?: (key: string) => void;
}) {
  return (
    <li className="flex items-center gap-2 rounded-md border border-edge bg-raised px-3 py-2">
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm leading-snug" title={paper.title}>
          {paper.title}
        </p>
        <p className="text-xs text-faint">
          {paper.year ?? "—"} · {paper.citedByCount.toLocaleString()} citations
        </p>
      </div>
      {paper.inLibraryKey ? (
        <button
          className="badge shrink-0 bg-ok-soft text-ok hover:opacity-80"
          title="Open this paper (already in your library)"
          onClick={() => paper.inLibraryKey && onOpenItem?.(paper.inLibraryKey)}
        >
          <IconCheck size={11} /> In library
        </button>
      ) : paper.doi ? (
        <button
          className="badge shrink-0 bg-inset text-muted hover:text-text"
          title={`Open https://doi.org/${paper.doi}`}
          onClick={() => void openUrl(`https://doi.org/${paper.doi}`)}
        >
          <IconExternalLink size={11} /> DOI
        </button>
      ) : (
        <span className="badge shrink-0 bg-inset text-faint">
          <IconLibrary size={11} /> Not in library
        </span>
      )}
    </li>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 pb-10 text-center">
      {children}
    </div>
  );
}
