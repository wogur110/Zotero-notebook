import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "../api";
import type { Item, Library, ProviderId, StoredSummary } from "../types";
import { collectionPath, formatAuthors, pathLabel } from "../lib/library";
import {
  IconAlert,
  IconExternalLink,
  IconFileText,
  IconFolderOpen,
  IconLoader,
  IconSparkles,
  IconX,
} from "./icons";

interface Props {
  item: Item;
  library: Library;
  defaultProvider: ProviderId;
  onClose: () => void;
}

export default function ItemDetailModal({
  item,
  library,
  defaultProvider,
  onClose,
}: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={item.title}
        className="card flex max-h-[84vh] w-[680px] max-w-[92vw] flex-col bg-surface"
        style={{ boxShadow: "var(--shadow-pop)" }}
      >
        <div className="flex items-center gap-2 px-6 pt-5">
          <span className="badge bg-inset text-muted">{item.itemType}</span>
          {item.year && (
            <span className="badge bg-inset text-muted">{item.year}</span>
          )}
          <div className="flex-1" />
          <button
            onClick={onClose}
            aria-label="Close"
            className="btn-ghost h-8 w-8 px-0!"
          >
            <IconX size={16} />
          </button>
        </div>

        <div className="px-6 pt-2">
          <h2 className="text-lg font-semibold leading-snug">{item.title}</h2>
          <p className="mt-1 text-sm text-muted">
            {formatAuthors(item.creators, 6)}
          </p>
        </div>

        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-6 pt-4">
          <MetadataGrid item={item} library={library} />
          <FileRow item={item} />
          <SummarySection item={item} defaultProvider={defaultProvider} />
          {item.abstractText && (
            <section>
              <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">
                Abstract
              </h3>
              <p className="text-sm leading-relaxed text-muted">
                {item.abstractText}
              </p>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-[11px] font-semibold uppercase tracking-wider text-faint">
        {label}
      </dt>
      <dd className="mt-0.5 text-sm">{children}</dd>
    </div>
  );
}

function MetadataGrid({ item, library }: { item: Item; library: Library }) {
  const collections = item.collectionKeys
    .map((k) => collectionPath(library, k))
    .filter((p) => p.length > 0);

  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-3">
      <Field label="Publication">
        <span className="text-muted">{item.publication ?? "—"}</span>
      </Field>
      <Field label="Added">
        <span className="text-muted">
          {item.dateAdded ? new Date(item.dateAdded).toLocaleDateString() : "—"}
        </span>
      </Field>
      {item.doi && (
        <Field label="DOI">
          <span className="flex items-center gap-1">
            <span className="truncate font-mono text-xs">{item.doi}</span>
            <button
              aria-label="Open DOI"
              title={`https://doi.org/${item.doi}`}
              className="btn-ghost h-6 w-6 shrink-0 px-0!"
              onClick={() => void openUrl(`https://doi.org/${item.doi}`)}
            >
              <IconExternalLink size={13} />
            </button>
          </span>
        </Field>
      )}
      {item.url && (
        <Field label="URL">
          <span className="flex items-center gap-1">
            <span className="truncate font-mono text-xs">{item.url}</span>
            <button
              aria-label="Open URL"
              title={item.url}
              className="btn-ghost h-6 w-6 shrink-0 px-0!"
              onClick={() => void openUrl(item.url!)}
            >
              <IconExternalLink size={13} />
            </button>
          </span>
        </Field>
      )}
      {item.tags.length > 0 && (
        <Field label="Tags">
          <span className="flex flex-wrap gap-1">
            {item.tags.map((t) => (
              <span key={t} className="badge bg-inset text-muted">
                {t}
              </span>
            ))}
          </span>
        </Field>
      )}
      {collections.length > 0 && (
        <Field label="Collections">
          <span className="flex flex-wrap gap-1">
            {collections.map((p, i) => (
              <span key={i} className="badge bg-accent-soft text-accent">
                {pathLabel(p)}
              </span>
            ))}
          </span>
        </Field>
      )}
    </dl>
  );
}

function FileRow({ item }: { item: Item }) {
  const [error, setError] = useState<string | null>(null);
  const run = (fn: () => Promise<void>) => () =>
    fn().catch((e) => setError(api.errorMessage(e)));

  return (
    <section className="rounded-lg border border-edge bg-raised p-3">
      <div className="flex items-center gap-2">
        <span className="text-faint">
          <IconFileText size={16} />
        </span>
        {item.attachment?.filePath ? (
          <span
            className="min-w-0 flex-1 truncate font-mono text-xs text-muted"
            title={item.attachment.filePath}
          >
            {item.attachment.filename ?? item.attachment.filePath}
          </span>
        ) : (
          <span className="flex-1 text-sm text-faint">No PDF on disk</span>
        )}
        <div className="flex shrink-0 gap-1.5">
          {item.attachment?.filePath && (
            <>
              <button
                className="btn-secondary py-1! text-xs"
                onClick={run(() => api.openItemPdf(item.key))}
              >
                <IconFileText size={13} /> Open PDF
              </button>
              <button
                className="btn-secondary py-1! text-xs"
                onClick={run(() => api.revealItemFile(item.key))}
              >
                <IconFolderOpen size={13} /> Show in Folder
              </button>
            </>
          )}
          <button
            className="btn-ghost py-1! text-xs"
            onClick={run(() => api.openInZotero(item.key))}
          >
            <IconExternalLink size={13} /> Open in Zotero
          </button>
        </div>
      </div>
      {error && (
        <p className="mt-2 rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger">
          {error}
        </p>
      )}
    </section>
  );
}

function SummarySection({
  item,
  defaultProvider,
}: {
  item: Item;
  defaultProvider: ProviderId;
}) {
  const [summary, setSummary] = useState<StoredSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .getSummary(item.key)
      .then((s) => {
        if (!cancelled) setSummary(s);
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [item.key]);

  const generate = useCallback(async () => {
    setGenerating(true);
    setError(null);
    try {
      setSummary(await api.summarizeItem(item.key, defaultProvider));
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setGenerating(false);
    }
  }, [item.key, defaultProvider]);

  return (
    <section className="rounded-lg border border-edge bg-raised p-4">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-accent">
          <IconSparkles size={15} />
        </span>
        <h3 className="text-sm font-semibold">AI Summary</h3>
        <div className="flex-1" />
        {summary && !generating && (
          <button className="btn-ghost py-0.5! text-xs" onClick={generate}>
            Regenerate
          </button>
        )}
      </div>

      {loading ? (
        <div className="space-y-2" aria-label="Loading summary">
          <div className="h-3 w-full animate-pulse rounded bg-inset" />
          <div className="h-3 w-5/6 animate-pulse rounded bg-inset" />
          <div className="h-3 w-2/3 animate-pulse rounded bg-inset" />
        </div>
      ) : summary ? (
        <>
          <p className="text-sm leading-relaxed">{summary.summary}</p>
          <p className="mt-2 flex flex-wrap items-center gap-2 text-xs text-faint">
            <span>
              {summary.provider} · {summary.model} ·{" "}
              {new Date(summary.createdAt).toLocaleString()}
            </span>
            {!summary.hadAbstract && (
              <span
                className="badge bg-warn-soft text-warn"
                title="No abstract was available in Zotero or from Crossref/Semantic Scholar/OpenAlex, so this summary is based on the title and venue only. Treat specifics with caution."
              >
                <IconAlert size={11} /> No abstract — title/venue only
              </span>
            )}
          </p>
        </>
      ) : (
        <div className="flex flex-col items-start gap-2.5">
          <p className="text-sm text-muted">
            No summary yet. Generate one from the paper's metadata and
            abstract.
          </p>
          <button
            className="btn-primary"
            onClick={generate}
            disabled={generating}
          >
            {generating ? (
              <>
                <IconLoader size={14} /> Summarizing…
              </>
            ) : (
              <>
                <IconSparkles size={14} /> Generate summary
              </>
            )}
          </button>
        </div>
      )}
      {generating && summary && (
        <p className="mt-2 flex items-center gap-1.5 text-xs text-muted">
          <IconLoader size={12} /> Regenerating…
        </p>
      )}
      {error && (
        <p className="mt-2 rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger">
          {error} — API keys can be added in Settings.
        </p>
      )}
    </section>
  );
}
