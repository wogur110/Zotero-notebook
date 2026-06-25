import { useEffect, useMemo, useState } from "react";
import type { Selection } from "../App";
import type {
  Library,
  ProviderId,
  ReadingState,
  ReadingStatus,
  StoredSummary,
} from "../types";
import {
  auditableItems,
  collectionPath,
  EMPTY_FILTER,
  filterItems,
  filterIsActive,
  type ItemFilter,
  itemsForCollection,
  queueItems,
  READING_STATUS_LABEL,
  starredItems,
} from "../lib/library";
import { buildReviewMarkdown, exportFileName } from "../lib/export";
import { saveMarkdown } from "../lib/exportActions";
import { errorMessage } from "../api";
import ItemTable from "../components/ItemTable";
import FilterBar from "../components/FilterBar";
import AuditFlow from "./AuditFlow";
import SummarizeFlow from "./SummarizeFlow";
import SynthesisFlow from "./SynthesisFlow";
import {
  IconAlert,
  IconChevronRight,
  IconDownload,
  IconFileText,
  IconLibrary,
  IconSparkles,
  IconStar,
  IconX,
} from "../components/icons";

interface Props {
  library: Library;
  selection: Selection;
  error: string | null;
  defaultProvider: ProviderId;
  /** Keys of items that already have a stored AI summary. */
  summarizedKeys: Set<string>;
  /** Stored AI summaries by item key (for the Markdown export). */
  summaries: Map<string, StoredSummary>;
  /** Reading state per item key (status badge column + the queue view). */
  readingStates: Map<string, ReadingState>;
  /** Toggle a paper's priority star from a list row. */
  onToggleStar?: (key: string) => void;
  /** Bulk actions over the current multi-selection. */
  onBulkStatus?: (keys: string[], status: ReadingStatus | null) => void;
  onBulkStarred?: (keys: string[], starred: boolean) => void;
  onBulkAddTags?: (keys: string[], tags: string[]) => void;
  onOpenItem: (key: string) => void;
  onRetry: () => void;
  onApplied: () => void;
  /** Notify the app that summaries changed (batch flow created some). */
  onSummarized: () => void;
}

export default function LibraryView({
  library,
  selection,
  error,
  defaultProvider,
  summarizedKeys,
  summaries,
  readingStates,
  onToggleStar,
  onBulkStatus,
  onBulkStarred,
  onBulkAddTags,
  onOpenItem,
  onRetry,
  onApplied,
  onSummarized,
}: Props) {
  const [auditing, setAuditing] = useState(false);
  const [summarizing, setSummarizing] = useState(false);
  const [synthesizing, setSynthesizing] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [filter, setFilter] = useState<ItemFilter>(EMPTY_FILTER);
  const [tagDraft, setTagDraft] = useState<string | null>(null);

  // A new sidebar selection is a fresh scope — drop any checked papers/filters.
  useEffect(() => {
    setSelectedKeys(new Set());
    setSynthesizing(false);
    setFilter(EMPTY_FILTER);
  }, [selection]);

  const items = useMemo(
    () =>
      selection.kind === "collection"
        ? itemsForCollection(library, selection.key)
        : selection.kind === "queue"
          ? queueItems(library, readingStates)
          : selection.kind === "starred"
            ? starredItems(library, readingStates)
            : library.items,
    [library, selection, readingStates],
  );
  const path =
    selection.kind === "collection"
      ? collectionPath(library, selection.key)
      : [];
  const scopeLabel =
    selection.kind === "queue"
      ? "Reading queue"
      : selection.kind === "starred"
        ? "Starred"
        : path.length === 0
          ? "All Papers"
          : path.join(" / ");
  // The list after the client-side filters; everything below acts on it.
  const filtered = useMemo(
    () => filterItems(items, filter, { readingStates, summarizedKeys }),
    [items, filter, readingStates, summarizedKeys],
  );
  const filtering = filterIsActive(filter);
  const auditable = useMemo(
    () => auditableItems(library, filtered),
    [library, filtered],
  );
  const unsummarized = useMemo(
    () => filtered.filter((i) => !summarizedKeys.has(i.key)),
    [filtered, summarizedKeys],
  );
  const selectedItems = useMemo(
    () => filtered.filter((i) => selectedKeys.has(i.key)),
    [filtered, selectedKeys],
  );
  const selectedKeyList = selectedItems.map((i) => i.key);
  // Synthesis / export scope: the checked subset, or the whole filtered view.
  const synthScope = selectedItems.length > 0 ? selectedItems : filtered;

  const toggleSelect = (key: string) =>
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const selectAll = (keys: string[], select: boolean) =>
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const k of keys) {
        if (select) next.add(k);
        else next.delete(k);
      }
      return next;
    });

  const exportReview = async () => {
    try {
      const md = buildReviewMarkdown({
        title: scopeLabel,
        generatedAt: new Date().toLocaleString(),
        items: synthScope,
        summaries,
      });
      const path = await saveMarkdown(exportFileName(scopeLabel), md);
      if (path) setExportMsg(`Saved to ${path}`);
    } catch (e) {
      setExportMsg(`Export failed: ${errorMessage(e)}`);
    }
    window.setTimeout(() => setExportMsg(null), 5000);
  };

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="card max-w-md p-6 text-center">
          <div className="mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full bg-danger-soft text-danger">
            <IconAlert size={20} />
          </div>
          <h2 className="mb-1 font-semibold">Could not load your library</h2>
          <p className="mb-4 text-sm text-muted">{error}</p>
          <button className="btn-secondary" onClick={onRetry}>
            Retry
          </button>
        </div>
      </div>
    );
  }

  if (auditing) {
    return (
      <AuditFlow
        library={library}
        items={auditable}
        scopeLabel={scopeLabel}
        defaultProvider={defaultProvider}
        onOpenItem={onOpenItem}
        onClose={() => setAuditing(false)}
        onApplied={onApplied}
      />
    );
  }

  if (summarizing) {
    return (
      <SummarizeFlow
        items={unsummarized}
        scopeLabel={scopeLabel}
        defaultProvider={defaultProvider}
        onClose={() => setSummarizing(false)}
        onSummarized={onSummarized}
      />
    );
  }

  if (synthesizing) {
    return (
      <SynthesisFlow
        items={synthScope}
        scopeLabel={
          selectedItems.length > 0
            ? `${selectedItems.length} selected`
            : scopeLabel
        }
        summaries={summaries}
        defaultProvider={defaultProvider}
        onClose={() => setSynthesizing(false)}
      />
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-baseline gap-3 px-6 pt-5 pb-3">
        <h1 className="flex items-center gap-1.5 text-lg font-semibold tracking-tight">
          {path.length === 0
            ? scopeLabel
            : path.map((seg, i) => (
                <span key={i} className="flex items-center gap-1.5">
                  {i > 0 && (
                    <span className="text-faint">
                      <IconChevronRight size={14} />
                    </span>
                  )}
                  {seg}
                </span>
              ))}
        </h1>
        <span className="text-sm text-muted">
          {filtered.length} {filtered.length === 1 ? "paper" : "papers"}
          {filtering && filtered.length !== items.length
            ? ` of ${items.length}`
            : ""}
        </span>
        <div className="flex-1" />
        {!library.writable && library.items.length > 0 && (
          <span className="badge bg-info-soft text-info">
            Read-only — install the Zotero plugin to enable classification
          </span>
        )}
        {filtered.length > 0 && (
          <button
            className="btn-secondary"
            title={
              selectedItems.length > 0
                ? `Ask AI across the ${selectedItems.length} selected ${selectedItems.length === 1 ? "paper" : "papers"} — overview, method comparison, or a question (metadata + abstracts)`
                : `Ask AI across all ${filtered.length} ${filtered.length === 1 ? "paper" : "papers"} in this view — overview, method comparison, or a question (metadata + abstracts)`
            }
            onClick={() => setSynthesizing(true)}
          >
            <IconLibrary size={14} />{" "}
            {selectedItems.length > 0
              ? `Synthesize ${selectedItems.length}`
              : "Synthesize"}
          </button>
        )}
        {filtered.length > 0 && (
          <button
            className="btn-secondary"
            disabled={unsummarized.length === 0}
            title={
              unsummarized.length === 0
                ? "Every paper in this view already has a summary"
                : `Quick-summarize the ${unsummarized.length} ${unsummarized.length === 1 ? "paper" : "papers"} in this view without a summary`
            }
            onClick={() => setSummarizing(true)}
          >
            <IconFileText size={14} /> Summarize {unsummarized.length}
          </button>
        )}
        {filtered.length > 0 && (
          <button
            className="btn-secondary"
            title={
              selectedItems.length > 0
                ? `Export the ${selectedItems.length} selected ${selectedItems.length === 1 ? "paper" : "papers"} as a Markdown review document (citations + AI summaries)`
                : "Export this view as a Markdown review document (citations + AI summaries)"
            }
            onClick={() => void exportReview()}
          >
            <IconDownload size={14} /> Export
          </button>
        )}
        {library.writable && (
          <button
            className="btn-secondary"
            disabled={auditable.length === 0}
            title={
              auditable.length === 0
                ? "No classified papers in this view to check"
                : `Ask AI to re-check the filing of ${auditable.length} ${auditable.length === 1 ? "paper" : "papers"} in this view`
            }
            onClick={() => setAuditing(true)}
          >
            <IconSparkles size={14} /> Check filing
          </button>
        )}
      </div>
      {items.length > 0 && (
        <FilterBar scope={items} filter={filter} onChange={setFilter} />
      )}
      {selectedItems.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 border-y border-edge bg-inset/40 px-6 py-1.5 text-xs">
          <span className="font-medium">{selectedItems.length} selected</span>
          <span className="text-faint">·</span>
          <span className="text-faint">Mark</span>
          {(["to_read", "reading", "read"] as ReadingStatus[]).map((s) => (
            <button
              key={s}
              className="btn-ghost py-0.5! text-xs"
              onClick={() => onBulkStatus?.(selectedKeyList, s)}
            >
              {READING_STATUS_LABEL[s]}
            </button>
          ))}
          <button
            className="btn-ghost py-0.5! text-xs"
            onClick={() => onBulkStatus?.(selectedKeyList, null)}
          >
            Clear status
          </button>
          <span className="text-edge">|</span>
          <button
            className="btn-ghost py-0.5! text-xs"
            onClick={() => onBulkStarred?.(selectedKeyList, true)}
          >
            <IconStar size={12} /> Star
          </button>
          <button
            className="btn-ghost py-0.5! text-xs"
            onClick={() => onBulkStarred?.(selectedKeyList, false)}
          >
            Unstar
          </button>
          {library.writable &&
            onBulkAddTags &&
            (tagDraft === null ? (
              <button
                className="btn-ghost py-0.5! text-xs"
                onClick={() => setTagDraft("")}
              >
                Add tag…
              </button>
            ) : (
              <form
                className="flex items-center gap-1"
                onSubmit={(e) => {
                  e.preventDefault();
                  const t = tagDraft.trim();
                  if (t) onBulkAddTags(selectedKeyList, [t]);
                  setTagDraft(null);
                }}
              >
                <input
                  className="input h-6 w-28 text-xs"
                  autoFocus
                  placeholder="tag name"
                  value={tagDraft}
                  onChange={(e) => setTagDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setTagDraft(null);
                  }}
                  aria-label="New tag"
                />
                <button type="submit" className="btn-secondary py-0.5! text-xs">
                  Add
                </button>
              </form>
            ))}
          <div className="flex-1" />
          <button
            className="btn-ghost py-0.5! text-xs"
            onClick={() => setSelectedKeys(new Set())}
            title="Clear the selected papers"
          >
            <IconX size={11} /> Clear {selectedItems.length} selected
          </button>
        </div>
      )}
      {exportMsg && (
        <div className="px-6 pb-1 text-xs text-muted">{exportMsg}</div>
      )}
      <div className="min-h-0 flex-1">
        <ItemTable
          items={filtered}
          onOpenItem={onOpenItem}
          selectedKeys={selectedKeys}
          onToggleSelect={toggleSelect}
          onSelectAll={selectAll}
          readingStates={readingStates}
          onToggleStar={onToggleStar}
          emptyTitle={
            filtering
              ? "No papers match the filters"
              : selection.kind === "queue"
                ? "Your reading queue is empty"
                : selection.kind === "starred"
                  ? "No starred papers yet"
                  : "No papers here"
          }
          emptyHint={
            filtering
              ? "Adjust or clear the filters above."
              : selection.kind === "queue"
                ? "Open a paper and set it to “To read” or “Reading” to add it here."
                : selection.kind === "starred"
                  ? "Click the ☆ on any paper (in the list or its popup) to star it."
                  : "Papers added to this collection in Zotero will show up after a refresh."
          }
        />
      </div>
    </div>
  );
}
