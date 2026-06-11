import { memo, useMemo, useState } from "react";
import type { Item } from "../types";
import { formatAuthors } from "../lib/library";
import { IconChevronRight, IconFileText, IconInbox } from "./icons";

type SortKey = "title" | "year" | "added";

interface Props {
  items: Item[];
  onOpenItem: (key: string) => void;
  emptyTitle?: string;
  emptyHint?: string;
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
}: Props) {
  const [sortKey, setSortKey] = useState<SortKey>("added");
  const [asc, setAsc] = useState(false);

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

  return (
    <div className="h-full overflow-y-auto">
      <div className="sticky top-0 z-10 flex items-center gap-3 border-b border-edge bg-bg px-6 py-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
        <button className="flex-1 text-left hover:text-muted" onClick={() => onSort("title")}>
          Title{arrow("title")}
        </button>
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
          <Row key={item.key} item={item} onOpen={onOpenItem} />
        ))}
      </ul>
    </div>
  );
}

const Row = memo(function Row({
  item,
  onOpen,
}: {
  item: Item;
  onOpen: (key: string) => void;
}) {
  return (
    <li className="group border-b border-edge">
      <button
        onClick={() => onOpen(item.key)}
        className="flex w-full items-center gap-3 px-6 py-2.5 text-left transition-colors hover:bg-inset"
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
