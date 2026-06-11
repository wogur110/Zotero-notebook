import { useEffect, useMemo, useRef, useState } from "react";
import type { Library } from "../types";
import { allPaths, pathLabel } from "../lib/library";
import { IconChevronRight, IconPlus } from "./icons";

interface Props {
  library: Library;
  value: string[];
  onChange: (path: string[]) => void;
}

/** Compact combobox for picking an existing collection path or typing a new
 * one ("A / B" creates a nested path). */
export default function PathPicker({ library, value, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const options = useMemo(() => {
    const seen = new Set<string>();
    const out: string[][] = [];
    for (const p of allPaths(library)) {
      if (p.length === 0) continue;
      if (p[0].trim().toLowerCase() === "unclassified") continue;
      const label = pathLabel(p);
      if (seen.has(label.toLowerCase())) continue;
      seen.add(label.toLowerCase());
      out.push(p);
    }
    out.sort((a, b) => pathLabel(a).localeCompare(pathLabel(b)));
    return out;
  }, [library]);

  const existingLabels = useMemo(
    () => new Set(options.map((p) => pathLabel(p).toLowerCase())),
    [options],
  );
  const isNew = !existingLabels.has(pathLabel(value).toLowerCase());

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return options;
    return options.filter((p) => pathLabel(p).toLowerCase().includes(q));
  }, [options, filter]);

  const exactMatch = existingLabels.has(filter.trim().toLowerCase());

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const pick = (path: string[]) => {
    onChange(path);
    setOpen(false);
    setFilter("");
  };

  const createFromFilter = () => {
    const segments = filter
      .split("/")
      .map((s) => s.trim())
      .filter(Boolean)
      .slice(0, 3);
    if (segments.length > 0) pick(segments);
  };

  return (
    <div ref={rootRef} className="relative min-w-0">
      <button
        onClick={() => setOpen((v) => !v)}
        className="input flex h-8 w-full items-center gap-1.5 text-left"
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        <span className="flex min-w-0 flex-1 items-center gap-1 truncate">
          {value.map((seg, i) => (
            <span key={i} className="flex min-w-0 items-center gap-1">
              {i > 0 && (
                <span className="shrink-0 text-faint">
                  <IconChevronRight size={11} />
                </span>
              )}
              <span className="truncate">{seg}</span>
            </span>
          ))}
        </span>
        {isNew && (
          <span className="badge shrink-0 bg-warn-soft text-warn">new</span>
        )}
      </button>

      {open && (
        <div className="card absolute left-0 top-9 z-20 w-full min-w-[280px] overflow-hidden bg-raised" style={{ boxShadow: "var(--shadow-pop)" }}>
          <div className="border-b border-edge p-2">
            <input
              autoFocus
              className="input h-7 text-xs"
              placeholder='Filter, or type a new path like "Topic / Subtopic"'
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !exactMatch && filter.trim()) {
                  createFromFilter();
                }
              }}
            />
          </div>
          <ul role="listbox" className="max-h-56 overflow-y-auto p-1">
            {filtered.map((p) => (
              <li key={pathLabel(p)}>
                <button
                  className="w-full truncate rounded px-2 py-1.5 text-left text-sm hover:bg-inset"
                  onClick={() => pick(p)}
                >
                  {pathLabel(p)}
                </button>
              </li>
            ))}
            {filter.trim() && !exactMatch && (
              <li>
                <button
                  className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm font-medium text-accent hover:bg-accent-soft"
                  onClick={createFromFilter}
                >
                  <IconPlus size={13} />
                  Create "{filter.trim()}"
                </button>
              </li>
            )}
            {filtered.length === 0 && !filter.trim() && (
              <li className="px-2 py-2 text-xs text-faint">
                No collections yet — type a name to create one.
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
