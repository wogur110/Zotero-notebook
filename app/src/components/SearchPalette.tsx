import { useEffect, useMemo, useRef, useState } from "react";
import Fuse from "fuse.js";
import type { Item, Library } from "../types";
import { collectionPath, formatAuthors, pathLabel } from "../lib/library";
import { IconFileText, IconSearch } from "./icons";

interface Props {
  open: boolean;
  library: Library;
  onClose: () => void;
  onOpenItem: (key: string) => void;
}

export default function SearchPalette({
  open,
  library,
  onClose,
  onOpenItem,
}: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const fuse = useMemo(
    () =>
      new Fuse(library.items, {
        keys: [
          { name: "title", weight: 2 },
          { name: "creators", weight: 1 },
          { name: "tags", weight: 1 },
          { name: "publication", weight: 1 },
          { name: "abstractText", weight: 0.5 },
        ],
        threshold: 0.35,
        ignoreLocation: true,
      }),
    [library.items],
  );

  const results: Item[] = useMemo(() => {
    if (!query.trim()) {
      return [...library.items]
        .sort((a, b) => (b.dateAdded ?? "").localeCompare(a.dateAdded ?? ""))
        .slice(0, 8);
    }
    return fuse.search(query).map((r) => r.item).slice(0, 30);
  }, [query, fuse, library.items]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
    }
  }, [open]);

  useEffect(() => setActive(0), [query]);

  useEffect(() => {
    const el = listRef.current?.children[active] as HTMLElement | undefined;
    el?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  if (!open) return null;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter" && results[active]) {
      onOpenItem(results[active].key);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 bg-black/40"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="presentation"
    >
      <div
        className="card mx-auto mt-[12vh] flex w-[600px] max-w-[92vw] flex-col overflow-hidden bg-surface"
        style={{ boxShadow: "var(--shadow-pop)" }}
        role="dialog"
        aria-modal="true"
        aria-label="Search papers"
        onKeyDown={onKeyDown}
      >
        <div className="flex items-center gap-2.5 border-b border-edge px-4 py-3">
          <span className="text-faint">
            <IconSearch size={16} />
          </span>
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search by title, author, tag, venue…"
            className="flex-1 bg-transparent text-sm outline-none placeholder:text-faint"
            aria-label="Search query"
          />
        </div>

        {!query.trim() && results.length > 0 && (
          <p className="px-4 pt-2 text-[11px] font-semibold uppercase tracking-wider text-faint">
            Recent
          </p>
        )}

        <ul ref={listRef} className="max-h-[50vh] overflow-y-auto p-1.5">
          {results.length === 0 ? (
            <li className="px-3 py-6 text-center text-sm text-faint">
              No papers match "{query}"
            </li>
          ) : (
            results.map((item, i) => {
              const firstPath = item.collectionKeys[0]
                ? collectionPath(library, item.collectionKeys[0])
                : [];
              return (
                <li key={item.key}>
                  <button
                    className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left ${
                      i === active ? "bg-inset" : ""
                    }`}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => onOpenItem(item.key)}
                  >
                    <span className="shrink-0 text-faint">
                      <IconFileText size={15} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">
                        {item.title}
                      </span>
                      <span className="block truncate text-xs text-muted">
                        {formatAuthors(item.creators)}
                        {item.year ? ` · ${item.year}` : ""}
                        {firstPath.length > 0 ? ` · ${pathLabel(firstPath)}` : ""}
                      </span>
                    </span>
                  </button>
                </li>
              );
            })
          )}
        </ul>

        <div className="border-t border-edge px-4 py-2 text-[11px] text-faint">
          ↑↓ navigate · ↵ open · esc close
        </div>
      </div>
    </div>
  );
}
