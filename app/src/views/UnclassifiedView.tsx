import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type {
  ClassificationProposal,
  Item,
  Library,
  MoveResult,
  ProgressEvent,
  ProviderId,
} from "../types";
import ItemTable from "../components/ItemTable";
import PathPicker from "../components/PathPicker";
import ProgressCard from "../components/ProgressCard";
import {
  IconAlert,
  IconArrowRight,
  IconCheck,
  IconChevronRight,
  IconSparkles,
} from "../components/icons";

interface Props {
  library: Library;
  items: Item[];
  writable: boolean;
  defaultProvider: ProviderId;
  onOpenItem: (key: string) => void;
  onApplied: () => void;
}

type Phase = "idle" | "classifying" | "review" | "applying" | "done";

interface ReviewRow {
  proposal: ClassificationProposal;
  path: string[];
  checked: boolean;
  expanded: boolean;
  /** AI-suggested tags with their per-row approval state (default on). */
  tags: { name: string; on: boolean }[];
}

interface FailedRow {
  itemKey: string;
  message: string;
}

export default function UnclassifiedView({
  library,
  items,
  writable,
  defaultProvider,
  onOpenItem,
  onApplied,
}: Props) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [rows, setRows] = useState<ReviewRow[]>([]);
  const [failures, setFailures] = useState<FailedRow[]>([]);
  const [results, setResults] = useState<MoveResult[]>([]);
  const [fatal, setFatal] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void)[]>([]);
  // Backend runs are not cancellable; a run id keeps events/results from an
  // abandoned run out of a newer one.
  const runIdRef = useRef(0);

  useEffect(
    () => () => {
      unlistenRef.current.forEach((f) => f());
      unlistenRef.current = [];
    },
    [],
  );

  const titleOf = (key: string) =>
    library.items.find((i) => i.key === key)?.title ?? key;

  const startClassify = async () => {
    const runId = ++runIdRef.current;
    setPhase("classifying");
    setFatal(null);
    setFailures([]);
    setProgress({ done: 0, total: items.length, itemKey: null, state: "running", message: null });
    const errors: FailedRow[] = [];
    const un = await api.onClassifyProgress((p) => {
      if (runIdRef.current !== runId) return; // event from an abandoned run
      setProgress(p);
      if (p.state === "error" && p.itemKey) {
        errors.push({ itemKey: p.itemKey, message: p.message ?? "failed" });
      }
    });
    unlistenRef.current.push(un);
    try {
      const proposals = await api.classifyItems(
        items.map((i) => i.key),
        defaultProvider,
      );
      if (runIdRef.current !== runId) return; // a newer run took over
      setRows(
        proposals.map((p) => ({
          proposal: p,
          path: p.proposedPath,
          checked: true,
          expanded: false,
          tags: (p.suggestedTags ?? []).map((name) => ({ name, on: true })),
        })),
      );
      setFailures(errors);
      setPhase("review");
    } catch (e) {
      if (runIdRef.current === runId) {
        setFatal(api.errorMessage(e));
        setPhase("idle");
      }
    } finally {
      un();
      unlistenRef.current = unlistenRef.current.filter((f) => f !== un);
    }
  };

  const startApply = async () => {
    const selected = rows.filter((r) => r.checked);
    if (selected.length === 0) return;
    const runId = ++runIdRef.current;
    setPhase("applying");
    setProgress({ done: 0, total: selected.length, itemKey: null, state: "running", message: null });
    const un = await api.onApplyProgress((p) => {
      if (runIdRef.current === runId) setProgress(p);
    });
    unlistenRef.current.push(un);
    try {
      const res = await api.applyClassifications(
        selected.map((r) => ({
          itemKey: r.proposal.itemKey,
          targetPath: r.path,
          addTags: r.tags.filter((t) => t.on).map((t) => t.name),
        })),
      );
      if (runIdRef.current !== runId) return;
      setResults(res);
      setPhase("done");
    } catch (e) {
      if (runIdRef.current === runId) {
        setFatal(api.errorMessage(e));
        setPhase("review");
      }
    } finally {
      un();
      unlistenRef.current = unlistenRef.current.filter((f) => f !== un);
    }
  };

  if (phase === "classifying" || phase === "applying") {
    const verb = phase === "classifying" ? "Analyzing" : "Moving";
    const pct = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
    return (
      <ProgressCard
        title={`${verb} paper ${Math.min((progress?.done ?? 0) + 1, progress?.total ?? 1)} of ${progress?.total ?? items.length}`}
        subtitle={progress?.itemKey ? titleOf(progress.itemKey) : "…"}
        pct={pct}
      />
    );
  }

  if (phase === "review") {
    const checkedCount = rows.filter((r) => r.checked).length;
    return (
      <div className="flex h-full flex-col">
        <div className="sticky top-0 z-10 flex items-center gap-3 border-b border-edge bg-bg px-6 py-3">
          <div>
            <h1 className="text-lg font-semibold tracking-tight">Review proposals</h1>
            <p className="text-xs text-muted">
              Edit the target collection per paper, untick anything you don't
              want to move, then apply.
            </p>
          </div>
          <div className="flex-1" />
          <button className="btn-ghost" onClick={() => setPhase("idle")}>
            Cancel
          </button>
          <button
            className="btn-primary"
            disabled={checkedCount === 0}
            onClick={startApply}
          >
            <IconCheck size={14} />
            Apply {checkedCount} {checkedCount === 1 ? "move" : "moves"}
          </button>
        </div>
        {fatal && (
          <p className="mx-6 mt-3 rounded-md bg-danger-soft px-3 py-2 text-sm text-danger">
            {fatal}
          </p>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <ul className="space-y-2">
            {rows.map((row, idx) => (
              <li key={row.proposal.itemKey} className="card p-3">
                <div className="flex items-center gap-3">
                  <input
                    type="checkbox"
                    className="h-4 w-4 shrink-0 accent-(--accent)"
                    checked={row.checked}
                    aria-label={`Include ${titleOf(row.proposal.itemKey)}`}
                    onChange={(e) =>
                      setRows((rs) =>
                        rs.map((r, i) =>
                          i === idx ? { ...r, checked: e.target.checked } : r,
                        ),
                      )
                    }
                  />
                  <button
                    onClick={() => onOpenItem(row.proposal.itemKey)}
                    className="min-w-0 flex-[1.2] truncate text-left text-sm font-medium hover:text-accent"
                    title={titleOf(row.proposal.itemKey)}
                  >
                    {titleOf(row.proposal.itemKey)}
                  </button>
                  <span className="shrink-0 text-faint">
                    <IconArrowRight size={14} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <PathPicker
                      library={library}
                      value={row.path}
                      onChange={(path) =>
                        setRows((rs) =>
                          rs.map((r, i) => (i === idx ? { ...r, path } : r)),
                        )
                      }
                    />
                  </div>
                  <span
                    className={`w-10 shrink-0 text-right text-xs ${
                      row.proposal.confidence < 0.5 ? "text-warn" : "text-faint"
                    }`}
                    title="Model confidence"
                  >
                    {Math.round(row.proposal.confidence * 100)}%
                  </span>
                  <button
                    aria-label="Show rationale"
                    className="btn-ghost h-7 w-7 shrink-0 px-0!"
                    onClick={() =>
                      setRows((rs) =>
                        rs.map((r, i) =>
                          i === idx ? { ...r, expanded: !r.expanded } : r,
                        ),
                      )
                    }
                  >
                    <IconChevronRight
                      size={14}
                      className={`transition-transform ${row.expanded ? "rotate-90" : ""}`}
                    />
                  </button>
                </div>
                {row.tags.length > 0 && (
                  <div className="mt-2 flex flex-wrap items-center gap-1.5 pl-7">
                    <span className="text-[11px] font-semibold uppercase tracking-wider text-faint">
                      Tags
                    </span>
                    {row.tags.map((t, ti) => (
                      <button
                        key={t.name}
                        aria-pressed={t.on}
                        title={
                          t.on
                            ? "Will be added as a Zotero tag — click to skip"
                            : "Skipped — click to add"
                        }
                        onClick={() =>
                          setRows((rs) =>
                            rs.map((r, i) =>
                              i === idx
                                ? {
                                    ...r,
                                    tags: r.tags.map((tt, tj) =>
                                      tj === ti ? { ...tt, on: !tt.on } : tt,
                                    ),
                                  }
                                : r,
                            ),
                          )
                        }
                        className={`badge cursor-pointer transition-colors ${
                          t.on
                            ? "bg-accent-soft text-accent"
                            : "bg-inset text-faint line-through"
                        }`}
                      >
                        {t.name}
                      </button>
                    ))}
                  </div>
                )}
                {row.expanded && (
                  <p className="mt-2 rounded-md bg-inset px-3 py-2 text-xs leading-relaxed text-muted">
                    {row.proposal.rationale || "No rationale provided."}
                  </p>
                )}
              </li>
            ))}
            {failures.map((f) => (
              <li
                key={f.itemKey}
                className="flex items-center gap-3 rounded-lg border border-edge bg-danger-soft p-3"
              >
                <span className="text-danger">
                  <IconAlert size={15} />
                </span>
                <span className="min-w-0 flex-1 truncate text-sm">
                  {titleOf(f.itemKey)}
                </span>
                <span className="truncate text-xs text-danger">{f.message}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    );
  }

  if (phase === "done") {
    const ok = results.filter((r) => r.ok);
    const failed = results.filter((r) => !r.ok);
    return (
      <Centered>
        <div className="card w-[480px] max-w-[90vw] p-6">
          <div
            className={`mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full ${
              failed.length === 0 ? "bg-ok-soft text-ok" : "bg-warn-soft text-warn"
            }`}
          >
            {failed.length === 0 ? <IconCheck size={20} /> : <IconAlert size={20} />}
          </div>
          <h2 className="text-center text-lg font-semibold">
            {ok.length} {ok.length === 1 ? "paper" : "papers"} classified
          </h2>
          {failed.length > 0 && (
            <div className="mt-3 space-y-1.5">
              <p className="text-sm text-muted">
                {failed.length} {failed.length === 1 ? "move" : "moves"} failed —
                these papers stay in Unclassified:
              </p>
              <ul className="max-h-40 space-y-1 overflow-y-auto">
                {failed.map((r) => (
                  <li
                    key={r.itemKey}
                    className="rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger"
                  >
                    <span className="font-medium">{titleOf(r.itemKey)}</span>
                    {r.error ? ` — ${r.error}` : ""}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <button
            className="btn-primary mt-5 w-full"
            onClick={() => {
              onApplied();
              setPhase("idle");
              setRows([]);
              setResults([]);
            }}
          >
            Back to library
          </button>
        </div>
      </Centered>
    );
  }

  // idle
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 px-6 pt-5 pb-3">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Unclassified</h1>
          <p className="text-xs text-muted">
            Papers with no collection yet. Let AI propose a place for each —
            you review before anything moves.
          </p>
        </div>
        <div className="flex-1" />
        <button
          className="btn-primary"
          disabled={!writable || items.length === 0}
          title={
            !writable
              ? "Install the Zotero plugin to enable moves (Settings → Zotero)"
              : undefined
          }
          onClick={startClassify}
        >
          <IconSparkles size={15} />
          Classify {items.length > 0 ? items.length : ""}{" "}
          {items.length === 1 ? "paper" : "papers"} with AI
        </button>
      </div>
      {fatal && (
        <p className="mx-6 mb-2 rounded-md bg-danger-soft px-3 py-2 text-sm text-danger">
          {fatal}
        </p>
      )}
      <div className="min-h-0 flex-1">
        {items.length === 0 ? (
          <Centered>
            <div className="text-center">
              <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-ok-soft text-ok">
                <IconCheck size={22} />
              </div>
              <p className="font-medium">Inbox zero</p>
              <p className="mt-1 text-sm text-muted">
                Every paper in your library has a collection.
              </p>
            </div>
          </Centered>
        ) : (
          <ItemTable items={items} onOpenItem={onOpenItem} />
        )}
      </div>
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-6">{children}</div>
  );
}
