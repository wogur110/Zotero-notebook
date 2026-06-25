// A compact filter row for the library list: reading status, starred, summary
// presence, PDF presence, tag, and year. Options for tag/year are derived from
// the current scope. Pure presentation — LibraryView applies the predicate.

import { useMemo } from "react";
import type { Item } from "../types";
import { type ItemFilter, filterIsActive } from "../lib/library";
import { IconFilter, IconX } from "./icons";

interface Props {
  /** The current scope (unfiltered) — used to derive tag/year options. */
  scope: Item[];
  filter: ItemFilter;
  onChange: (next: ItemFilter) => void;
}

const SELECT =
  "h-7 rounded-md border border-edge bg-raised px-1.5 text-xs text-text";

export default function FilterBar({ scope, filter, onChange }: Props) {
  const tags = useMemo(
    () =>
      [...new Set(scope.flatMap((i) => i.tags))].sort((a, b) =>
        a.localeCompare(b, undefined, { sensitivity: "base" }),
      ),
    [scope],
  );
  const years = useMemo(
    () =>
      [...new Set(scope.map((i) => i.year).filter((y): y is number => y != null))].sort(
        (a, b) => b - a,
      ),
    [scope],
  );

  const set = (patch: Partial<ItemFilter>) => onChange({ ...filter, ...patch });
  const active = filterIsActive(filter);

  return (
    <div className="flex flex-wrap items-center gap-2 px-6 pb-2 text-xs">
      <span className="flex items-center gap-1 text-faint">
        <IconFilter size={12} /> Filter
      </span>

      <select
        className={SELECT}
        aria-label="Reading status filter"
        value={filter.status ?? "any"}
        onChange={(e) => {
          const v = e.target.value;
          set({
            status:
              v === "any"
                ? null
                : v === "none"
                  ? "none"
                  : (v as ItemFilter["status"]),
          });
        }}
      >
        <option value="any">Any status</option>
        <option value="to_read">To read</option>
        <option value="reading">Reading</option>
        <option value="read">Read</option>
        <option value="none">Untracked</option>
      </select>

      <button
        className={`badge ${filter.starred ? "bg-accent-soft text-accent" : "bg-inset text-muted hover:text-text"}`}
        aria-pressed={filter.starred}
        onClick={() => set({ starred: !filter.starred })}
      >
        ★ Starred
      </button>

      <select
        className={SELECT}
        aria-label="Summary filter"
        value={filter.summary}
        onChange={(e) =>
          set({ summary: e.target.value as ItemFilter["summary"] })
        }
      >
        <option value="any">Any summary</option>
        <option value="has">Has summary</option>
        <option value="missing">No summary</option>
      </select>

      <button
        className={`badge ${filter.pdf ? "bg-accent-soft text-accent" : "bg-inset text-muted hover:text-text"}`}
        aria-pressed={filter.pdf}
        onClick={() => set({ pdf: !filter.pdf })}
      >
        Has PDF
      </button>

      {tags.length > 0 && (
        <select
          className={`${SELECT} max-w-[160px]`}
          aria-label="Tag filter"
          value={filter.tag ?? ""}
          onChange={(e) => set({ tag: e.target.value || null })}
        >
          <option value="">Any tag</option>
          {tags.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
      )}

      {years.length > 0 && (
        <select
          className={SELECT}
          aria-label="Year filter"
          value={filter.year ?? ""}
          onChange={(e) =>
            set({ year: e.target.value ? Number(e.target.value) : null })
          }
        >
          <option value="">Any year</option>
          {years.map((y) => (
            <option key={y} value={y}>
              {y}
            </option>
          ))}
        </select>
      )}

      {active && (
        <button
          className="btn-ghost py-0.5! text-xs"
          onClick={() =>
            onChange({
              status: null,
              starred: false,
              summary: "any",
              pdf: false,
              tag: null,
              year: null,
            })
          }
        >
          <IconX size={11} /> Clear filters
        </button>
      )}
    </div>
  );
}
