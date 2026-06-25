import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "../api";
import type {
  Item,
  Library,
  ProviderId,
  ReadingState,
  ReadingStatus,
  StoredSummary,
} from "../types";
import {
  collectionPath,
  formatAuthors,
  pathLabel,
  READING_STATUS_LABEL,
} from "../lib/library";
import ChatPanel from "./ChatPanel";
import CitationPanel from "./CitationPanel";
import {
  IconAlert,
  IconBookmark,
  IconCheck,
  IconExternalLink,
  IconFileText,
  IconFolderOpen,
  IconLoader,
  IconShare2,
  IconSparkles,
  IconStar,
  IconX,
} from "./icons";

interface Props {
  item: Item;
  library: Library;
  defaultProvider: ProviderId;
  readingState: ReadingState | null;
  onReadingChanged: (next: ReadingState | null) => void;
  /** Open another item (e.g. an in-library reference) in this modal. */
  onOpenItem?: (key: string) => void;
  onClose: () => void;
}

type Tab = "overview" | "chat" | "references";

export default function ItemDetailModal({
  item,
  library,
  defaultProvider,
  readingState,
  onReadingChanged,
  onOpenItem,
  onClose,
}: Props) {
  const [tab, setTab] = useState<Tab>("overview");

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
        className="card flex h-[84vh] w-[680px] max-w-[92vw] flex-col bg-surface"
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

        <div
          className="mt-3 flex gap-1 border-b border-edge px-6"
          role="tablist"
        >
          <TabButton
            active={tab === "overview"}
            onClick={() => setTab("overview")}
          >
            Overview
          </TabButton>
          <TabButton active={tab === "chat"} onClick={() => setTab("chat")}>
            <IconSparkles size={13} /> Ask AI
          </TabButton>
          <TabButton
            active={tab === "references"}
            onClick={() => setTab("references")}
          >
            <IconShare2 size={13} /> References
          </TabButton>
        </div>

        {tab === "overview" ? (
          <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-6 pt-4">
            <MetadataGrid item={item} library={library} />
            <FileRow item={item} />
            <ReadingStatusSection
              item={item}
              readingState={readingState}
              onChanged={onReadingChanged}
            />
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
        ) : tab === "chat" ? (
          <ChatPanel item={item} defaultProvider={defaultProvider} />
        ) : (
          <CitationPanel item={item} onOpenItem={onOpenItem} />
        )}
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-sm font-medium transition-colors ${
        active
          ? "border-accent text-accent"
          : "border-transparent text-muted hover:text-text"
      }`}
    >
      {children}
    </button>
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

const STATUS_OPTIONS: ReadingStatus[] = ["to_read", "reading", "read"];

function ReadingStatusSection({
  item,
  readingState,
  onChanged,
}: {
  item: Item;
  readingState: ReadingState | null;
  onChanged: (next: ReadingState | null) => void;
}) {
  const status = readingState?.status ?? null;
  const starred = readingState?.starred ?? false;
  // Seeded once; the modal remounts per item (keyed), so this stays in sync.
  const [note, setNote] = useState(readingState?.note ?? "");
  const [error, setError] = useState<string | null>(null);

  const persist = async (
    nextStatus: ReadingStatus | null,
    nextStarred: boolean,
    nextNote: string,
  ) => {
    setError(null);
    try {
      onChanged(
        await api.setReadingState(item.key, nextStatus, nextStarred, nextNote),
      );
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const pickStatus = (s: ReadingStatus) =>
    void persist(status === s ? null : s, starred, note);
  const toggleStar = () => void persist(status, !starred, note);
  const saveNote = () => {
    if (note !== (readingState?.note ?? "")) void persist(status, starred, note);
  };
  const clear = () => {
    setNote("");
    void persist(null, false, "");
  };

  const tracked = status !== null || starred || note.trim() !== "";

  return (
    <section className="rounded-lg border border-edge bg-raised p-4">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-accent">
          <IconBookmark size={15} />
        </span>
        <h3 className="text-sm font-semibold">Reading status</h3>
        <div className="flex-1" />
        {tracked && (
          <button className="btn-ghost py-0.5! text-xs" onClick={clear}>
            Clear
          </button>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <div className="inline-flex overflow-hidden rounded-md border border-edge">
          {STATUS_OPTIONS.map((s) => (
            <button
              key={s}
              onClick={() => pickStatus(s)}
              aria-pressed={status === s}
              className={`px-2.5 py-1 text-xs font-medium transition-colors ${
                status === s
                  ? "bg-accent text-accent-text"
                  : "text-muted hover:bg-inset"
              }`}
            >
              {READING_STATUS_LABEL[s]}
            </button>
          ))}
        </div>
        <button
          onClick={toggleStar}
          aria-pressed={starred}
          title="Mark as priority"
          className={`btn-secondary py-1! text-xs ${starred ? "text-accent" : ""}`}
        >
          <IconStar
            size={13}
            fill={starred ? "currentColor" : "none"}
            strokeWidth={starred ? 0 : 1.5}
          />
          {starred ? "Priority" : "Set priority"}
        </button>
      </div>
      <textarea
        className="input mt-2 resize-none"
        rows={2}
        placeholder="Personal note (e.g. why it matters, what to check)…"
        value={note}
        onChange={(e) => setNote(e.target.value)}
        onBlur={saveNote}
      />
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
  const [noteState, setNoteState] = useState<"idle" | "saving" | "saved">("idle");

  const saveNote = async () => {
    setNoteState("saving");
    setError(null);
    try {
      await api.saveSummaryNote(item.key);
      setNoteState("saved");
      setTimeout(() => setNoteState("idle"), 2000);
    } catch (e) {
      setNoteState("idle");
      setError(api.errorMessage(e));
    }
  };

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

  const generate = useCallback(
    async (useFulltext: boolean) => {
      setGenerating(true);
      setError(null);
      try {
        setSummary(await api.summarizeItem(item.key, defaultProvider, useFulltext));
      } catch (e) {
        setError(api.errorMessage(e));
      } finally {
        setGenerating(false);
      }
    },
    [item.key, defaultProvider],
  );

  const fulltextTooltip =
    "Reads the whole PDF (up to ~80k characters) — better summary, but a noticeably larger AI request than the default.";

  return (
    <section className="rounded-lg border border-edge bg-raised p-4">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-accent">
          <IconSparkles size={15} />
        </span>
        <h3 className="text-sm font-semibold">AI Summary</h3>
        <div className="flex-1" />
        {summary && !generating && (
          <>
            <button
              className="btn-ghost py-0.5! text-xs"
              onClick={() => void generate(false)}
            >
              Regenerate
            </button>
            <button
              className="btn-ghost py-0.5! text-xs"
              title={fulltextTooltip}
              onClick={() => void generate(true)}
            >
              <IconFileText size={12} /> From full text
            </button>
            <button
              className="btn-ghost py-0.5! text-xs"
              title="Save this summary as a child note on the Zotero item (updates the existing note in place)"
              disabled={noteState === "saving"}
              onClick={() => void saveNote()}
            >
              {noteState === "saved" ? (
                <>
                  <IconCheck size={12} /> Saved
                </>
              ) : noteState === "saving" ? (
                <>
                  <IconLoader size={12} /> Saving…
                </>
              ) : (
                <>
                  <IconExternalLink size={12} /> Save to Zotero
                </>
              )}
            </button>
          </>
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
            {summary.source === "fulltext" && (
              <span
                className="badge bg-info-soft text-info"
                title="This summary was generated from the paper's extracted full text."
              >
                <IconFileText size={11} /> Full text
              </span>
            )}
            {summary.source === "metadata" && (
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
            No summary yet. The default uses the metadata and abstract; the
            full-text option reads the whole PDF for a deeper summary.
          </p>
          <div className="flex gap-2">
            <button
              className="btn-primary"
              onClick={() => void generate(false)}
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
            <button
              className="btn-secondary"
              onClick={() => void generate(true)}
              disabled={generating}
              title={fulltextTooltip}
            >
              <IconFileText size={14} /> Full-text summary
            </button>
          </div>
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
