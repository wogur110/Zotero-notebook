import { useMemo } from "react";
import type { Selection } from "../App";
import type { Library } from "../types";
import { collectionPath, itemsForCollection } from "../lib/library";
import ItemTable from "../components/ItemTable";
import { IconAlert, IconChevronRight } from "../components/icons";

interface Props {
  library: Library;
  selection: Selection;
  error: string | null;
  onOpenItem: (key: string) => void;
  onRetry: () => void;
}

export default function LibraryView({
  library,
  selection,
  error,
  onOpenItem,
  onRetry,
}: Props) {
  const items = useMemo(
    () =>
      selection.kind === "collection"
        ? itemsForCollection(library, selection.key)
        : library.items,
    [library, selection],
  );
  const path =
    selection.kind === "collection"
      ? collectionPath(library, selection.key)
      : [];

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

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-baseline gap-3 px-6 pt-5 pb-3">
        <h1 className="flex items-center gap-1.5 text-lg font-semibold tracking-tight">
          {path.length === 0
            ? "All Papers"
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
      </div>
      <div className="min-h-0 flex-1">
        <ItemTable
          items={items}
          onOpenItem={onOpenItem}
          emptyTitle="No papers here"
          emptyHint="Papers added to this collection in Zotero will show up after a refresh."
        />
      </div>
    </div>
  );
}
