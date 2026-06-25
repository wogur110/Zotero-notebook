import { memo, useMemo, useRef, useState } from "react";
import type { Item, ReadingState } from "../types";
import { formatAuthors, READING_STATUS_LABEL } from "../lib/library";
import {
  IconChevronRight,
  IconFileText,
  IconInbox,
  IconStar,
} from "./icons";

type SortKey = "title" | "year" | "added";

interface Props {
  items: Item[];
  onOpenItem: (key: string) => void;
  emptyTitle?: string;
  emptyHint?: string;
  /**
   * When provided (together with the handlers), a selection checkbox column is
   * shown so the parent can act on an ad-hoc subset (e.g. multi-paper
   * synthesis). Omit for a plain open-only list.
   */
  selectedKeys?: Set<string>;
  onToggleSelect?: (key: string) => void;
  onSelectAll?: (keys: string[], select: boolean) => void;
  /** When provided, a reading-status/star column is shown. */
  readingStates?: Map<string, ReadingState>;
}

export function relativeDate(iso: string | null): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const days = Math.floor((Date.now() - then) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "1d ago";
  if (days < 30) return `${days}d ago`;
  if (days < 365) return `${Math.floor(days / 30)}mo ago`;
  return `${Math.floor(days / 365)}y ago`;
}

export default function ItemTable({
  items,
  onOpenItem,
  emptyTitle = "No papers here",
  emptyHint,
  selectedKeys,
  onToggleSelect,
  onSelectAll,
  readingStates,
}: Props) {
  const [sortKey, setSortKey] = useState<SortKey>("added");
  const [asc, setAsc] = useState(false);
  const selectable = !!selectedKeys && !!onToggleSelect && !!onSelectAll;
  const showStatus = !!readingStates;

  const sorted = useMemo(() => {
    const copy = [...items];
    copy.sort((a, b) => {
      let cmp = 0;
      if (sortKey === "title") {
        cmp = a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
      } else if (sortKey === "year") {
        cmp = (a.year ?? 0) - (b.year ?? 0);
      } else {
        cmp = (a.dateAdded ?? "").localeCompare(b.dateAdded ?? "");
      }
      return asc ? cmp : -cmp;
    });
    return copy;
  }, [items, sortKey, asc]);

  const onSort = (key: SortKey) => {
    if (key === sortKey) setAsc((v) => !v);
    else {
      setSortKey(key);
      setAsc(key === "title");
    }
  };

  if (items.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-6 pb-16 text-center">
        <div className="flex h-12 w-12 items-center justify-center rounded-full bg-inset text-faint">
          <IconInbox size={22} />
        </div>
        <p className="font-medium">{emptyTitle}</p>
        {emptyHint && <p className="max-w-sm text-sm text-muted">{emptyHint}</p>}
      </div>
    );
  }

  const arrow = (key: SortKey) =>
    sortKey === key ? (asc ? " ↑" : " ↓") : "";

  const allSelected =
    selectable && sorted.every((i) => selectedKeys!.has(i.key));
  const someSelected =
    selectable && !allSelected && sorted.some((i) => selectedKeys!.has(i.key));

  return (
    <div className="h-full overflow-y-auto">
      <div className="sticky top-0 z-10 flex items-center gap-3 border-b border-edge bg-bg pr-6 py-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
        {selectable && (
          <span className="flex shrink-0 items-center pl-6">
            <SelectAllCheckbox
              checked={allSelected}
              indeterminate={someSelected}
              onChange={() =>
                onSelectAll!(
                  sorted.map((i) => i.key),
                  !allSelected,
                )
              }
            />
          </span>
        )}
        <button
          className={`flex-1 text-left hover:text-muted ${selectable ? "" : "pl-6"}`}
          onClick={() => onSort("title")}
        >
          Title{arrow("title")}
        </button>
        {showStatus && <span className="w-[92px] shrink-0">Status</span>}
        <button className="w-12 text-left hover:text-muted" onClick={() => onSort("year")}>
          Year{arrow("year")}
        </button>
        <button className="w-20 text-left hover:text-muted" onClick={() => onSort("added")}>
          Added{arrow("added")}
        </button>
        <span className="w-10" aria-hidden />
      </div>
      <ul>
        {sorted.map((item) => (
          <Row
            key={item.key}
            item={item}
            onOpen={onOpenItem}
            selectable={selectable}
            selected={selectable ? selectedKeys!.has(item.key) : false}
            onToggleSelect={onToggleSelect}
            showStatus={showStatus}
            state={readingStates?.get(item.key)}
          />
        ))}
      </ul>
    </div>
  );
}

/** A checkbox that supports the indeterminate ("some selected") visual. */
function SelectAllCheckbox({
  checked,
  indeterminate,
  onChange,
}: {
  checked: boolean;
  indeterminate: boolean;
  onChange: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  if (ref.current) ref.current.indeterminate = indeterminate;
  return (
    <input
      ref={ref}
      type="checkbox"
      aria-label="Select all papers in this view"
      className="h-4 w-4 cursor-pointer"
      style={{ accentColor: "var(--accent)" }}
      checked={checked}
      onChange={onChange}
    />
  );
}

const STATUS_STYLE: Record<ReadingState["status"], string> = {
  to_read: "bg-inset text-muted",
  reading: "bg-accent-soft text-accent",
  read: "bg-ok-soft text-ok",
};

/** Compact reading-status badge + priority star for a table row. */
function StatusCell({ state }: { state?: ReadingState }) {
  if (!state) return <span className="w-[92px] shrink-0" aria-hidden />;
  return (
    <span className="flex w-[92px] shrink-0 items-center gap-1">
      {state.starred && (
        <span className="text-accent" title="Priority">
          <IconStar size={12} fill="currentColor" strokeWidth={0} />
        </span>
      )}
      <span className={`badge ${STATUS_STYLE[state.status]}`}>
        {READING_STATUS_LABEL[state.status]}
      </span>
    </span>
  );
}

const Row = memo(function Row({
  item,
  onOpen,
  selectable,
  selected,
  onToggleSelect,
  showStatus,
  state,
}: {
  item: Item;
  onOpen: (key: string) => void;
  selectable: boolean;
  selected: boolean;
  onToggleSelect?: (key: string) => void;
  showStatus: boolean;
  state?: ReadingState;
}) {
  return (
    <li
      className={`group flex items-center border-b border-edge transition-colors ${
        selected ? "bg-accent-soft/40" : ""
      }`}
    >
      {selectable && (
        <label className="flex shrink-0 cursor-pointer items-center py-2.5 pl-6 pr-1">
          <input
            type="checkbox"
            aria-label={`Select ${item.title}`}
            className="h-4 w-4 cursor-pointer"
            style={{ accentColor: "var(--accent)" }}
            checked={selected}
            onChange={() => onToggleSelect?.(item.key)}
          />
        </label>
      )}
      <button
        onClick={() => onOpen(item.key)}
        className={`flex min-w-0 flex-1 items-center gap-3 py-2.5 pr-6 text-left transition-colors hover:bg-inset ${
          selectable ? "pl-2" : "pl-6"
        }`}
      >
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium leading-snug">
            {item.title}
          </p>
          <p className="truncate text-xs text-muted">
            {formatAuthors(item.creators)}
            {item.publication ? ` · ${item.publication}` : ""}
          </p>
        </div>
        {showStatus && <StatusCell state={state} />}
        <span className="w-12 shrink-0 text-sm text-muted">
          {item.year ?? "—"}
        </span>
        <span className="w-20 shrink-0 text-xs text-faint">
          {relativeDate(item.dateAdded)}
        </span>
        <span className="flex w-10 shrink-0 items-center justify-end gap-1">
          {item.attachment && (
            <span
              className="text-faint"
              title={item.attachment.filename ?? "PDF attached"}
            >
              <IconFileText size={14} />
            </span>
          )}
          <span className="text-faint opacity-0 transition-opacity group-hover:opacity-100">
            <IconChevronRight size={14} />
          </span>
        </span>
      </button>
    </li>
  );
});
