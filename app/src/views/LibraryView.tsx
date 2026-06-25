import { useEffect, useMemo, useState } from "react";
import type { Selection } from "../App";
import type { Library, ProviderId, ReadingState } from "../types";
import {
  auditableItems,
  collectionPath,
  itemsForCollection,
  queueItems,
  starredItems,
} from "../lib/library";
import ItemTable from "../components/ItemTable";
import AuditFlow from "./AuditFlow";
import SummarizeFlow from "./SummarizeFlow";
import SynthesisFlow from "./SynthesisFlow";
import {
  IconAlert,
  IconChevronRight,
  IconFileText,
  IconLibrary,
  IconSparkles,
} from "../components/icons";

interface Props {
  library: Library;
  selection: Selection;
  error: string | null;
  defaultProvider: ProviderId;
  /** Keys of items that already have a stored AI summary. */
  summarizedKeys: Set<string>;
  /** Reading state per item key (status badge column + the queue view). */
  readingStates: Map<string, ReadingState>;
  /** Toggle a paper's priority star from a list row. */
  onToggleStar?: (key: string) => void;
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
  readingStates,
  onToggleStar,
  onOpenItem,
  onRetry,
  onApplied,
  onSummarized,
}: Props) {
  const [auditing, setAuditing] = useState(false);
  const [summarizing, setSummarizing] = useState(false);
  const [synthesizing, setSynthesizing] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());

  // A new sidebar selection is a fresh scope — drop any checked papers.
  useEffect(() => {
    setSelectedKeys(new Set());
    setSynthesizing(false);
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
  const auditable = useMemo(
    () => auditableItems(library, items),
    [library, items],
  );
  const unsummarized = useMemo(
    () => items.filter((i) => !summarizedKeys.has(i.key)),
    [items, summarizedKeys],
  );
  const selectedItems = useMemo(
    () => items.filter((i) => selectedKeys.has(i.key)),
    [items, selectedKeys],
  );
  // Synthesis scope: the checked subset, or the whole current view.
  const synthScope = selectedItems.length > 0 ? selectedItems : items;

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
          {items.length} {items.length === 1 ? "paper" : "papers"}
        </span>
        <div className="flex-1" />
        {!library.writable && library.items.length > 0 && (
          <span className="badge bg-info-soft text-info">
            Read-only — install the Zotero plugin to enable classification
          </span>
        )}
        {selectedItems.length > 0 && (
          <button
            className="btn-ghost text-xs"
            onClick={() => setSelectedKeys(new Set())}
            title="Clear the selected papers"
          >
            Clear {selectedItems.length} selected
          </button>
        )}
        {items.length > 0 && (
          <button
            className="btn-secondary"
            title={
              selectedItems.length > 0
                ? `Ask AI across the ${selectedItems.length} selected ${selectedItems.length === 1 ? "paper" : "papers"} — overview, method comparison, or a question (metadata + abstracts)`
                : `Ask AI across all ${items.length} ${items.length === 1 ? "paper" : "papers"} in this view — overview, method comparison, or a question (metadata + abstracts)`
            }
            onClick={() => setSynthesizing(true)}
          >
            <IconLibrary size={14} />{" "}
            {selectedItems.length > 0
              ? `Synthesize ${selectedItems.length}`
              : "Synthesize"}
          </button>
        )}
        {items.length > 0 && (
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
      <div className="min-h-0 flex-1">
        <ItemTable
          items={items}
          onOpenItem={onOpenItem}
          selectedKeys={selectedKeys}
          onToggleSelect={toggleSelect}
          onSelectAll={selectAll}
          readingStates={readingStates}
          onToggleStar={onToggleStar}
          emptyTitle={
            selection.kind === "queue"
              ? "Your reading queue is empty"
              : selection.kind === "starred"
                ? "No starred papers yet"
                : "No papers here"
          }
          emptyHint={
            selection.kind === "queue"
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
