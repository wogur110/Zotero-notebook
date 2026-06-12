// Re-check the filing of already-classified papers: the LLM flags papers
// whose current collection doesn't fit and proposes a better one; nothing
// moves until the user reviews and applies. Scope = whatever list the user
// was looking at when they pressed the button.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type {
  AuditProposal,
  Item,
  Library,
  MoveResult,
  ProgressEvent,
  ProviderId,
} from "../types";
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
  /** Auditable items in the current view (≥1 non-Unclassified membership). */
  items: Item[];
  /** Label of the scope shown in the confirm step, e.g. "All Papers". */
  scopeLabel: string;
  defaultProvider: ProviderId;
  onOpenItem: (key: string) => void;
  onClose: () => void;
  onApplied: () => void;
}

type Phase = "confirm" | "scanning" | "review" | "applying" | "done";

interface ReviewRow {
  proposal: AuditProposal;
  path: string[];
  checked: boolean;
  expanded: boolean;
}

interface FailedRow {
  itemKey: string;
  message: string;
}

export default function AuditFlow({
  library,
  items,
  scopeLabel,
  defaultProvider,
  onOpenItem,
  onClose,
  onApplied,
}: Props) {
  const [phase, setPhase] = useState<Phase>("confirm");
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [rows, setRows] = useState<ReviewRow[]>([]);
  const [failures, setFailures] = useState<FailedRow[]>([]);
  const [scannedCount, setScannedCount] = useState(0);
  const [results, setResults] = useState<MoveResult[]>([]);
  const [fatal, setFatal] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void)[]>([]);
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

  const startScan = async () => {
    const runId = ++runIdRef.current;
    setPhase("scanning");
    setFatal(null);
    setFailures([]);
    setProgress({ done: 0, total: items.length, itemKey: null, state: "running", message: null });
    const errors: FailedRow[] = [];
    const un = await api.onAuditProgress((p) => {
      if (runIdRef.current !== runId) return;
      setProgress(p);
      if (p.state === "error" && p.itemKey) {
        errors.push({ itemKey: p.itemKey, message: p.message ?? "failed" });
      }
    });
    unlistenRef.current.push(un);
    try {
      const proposals = await api.auditItems(
        items.map((i) => i.key),
        defaultProvider,
      );
      if (runIdRef.current !== runId) return;
      setRows(
        proposals.map((p) => ({
          proposal: p,
          path: p.proposedPath,
          checked: true,
          expanded: false,
        })),
      );
      setFailures(errors);
      setScannedCount(items.length - errors.length);
      setPhase("review");
    } catch (e) {
      if (runIdRef.current === runId) {
        setFatal(api.errorMessage(e));
        setPhase("confirm");
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
          removeCollectionKeys: r.proposal.currentKeys,
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

  if (phase === "confirm") {
    return (
      <Centered>
        <div className="card w-[480px] max-w-[90vw] p-6">
          <div className="mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full bg-accent-soft text-accent">
            <IconSparkles size={20} />
          </div>
          <h2 className="text-center text-lg font-semibold">
            Check filing of {items.length}{" "}
            {items.length === 1 ? "paper" : "papers"}?
          </h2>
          <p className="mt-2 text-center text-sm text-muted">
            Scope: <span className="font-medium text-text">{scopeLabel}</span>.
            Each paper is one AI request ({items.length} total). The model is
            conservative — it only flags papers where no current collection
            fits, and nothing moves until you approve.
          </p>
          {fatal && (
            <p className="mt-3 rounded-md bg-danger-soft px-3 py-2 text-sm text-danger">
              {fatal}
            </p>
          )}
          <div className="mt-5 flex justify-center gap-2">
            <button className="btn-secondary" onClick={onClose}>
              Cancel
            </button>
            <button className="btn-primary" onClick={startScan}>
              <IconSparkles size={14} /> Start checking
            </button>
          </div>
        </div>
      </Centered>
    );
  }

  if (phase === "scanning" || phase === "applying") {
    const verb = phase === "scanning" ? "Checking" : "Moving";
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
    const fineCount = Math.max(scannedCount - rows.length, 0);
    return (
      <div className="flex h-full flex-col">
        <div className="sticky top-0 z-10 flex items-center gap-3 border-b border-edge bg-bg px-6 py-3">
          <div>
            <h1 className="text-lg font-semibold tracking-tight">
              Filing check results
            </h1>
            <p className="text-xs text-muted">
              {fineCount} of {scannedCount} look correctly filed.{" "}
              {rows.length > 0
                ? "Review the flagged papers below — edit targets, untick what should stay."
                : ""}
            </p>
          </div>
          <div className="flex-1" />
          <button className="btn-ghost" onClick={onClose}>
            Close
          </button>
          {rows.length > 0 && (
            <button
              className="btn-primary"
              disabled={checkedCount === 0}
              onClick={startApply}
            >
              <IconCheck size={14} />
              Move {checkedCount} {checkedCount === 1 ? "paper" : "papers"}
            </button>
          )}
        </div>
        {fatal && (
          <p className="mx-6 mt-3 rounded-md bg-danger-soft px-3 py-2 text-sm text-danger">
            {fatal}
          </p>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          {rows.length === 0 && failures.length === 0 ? (
            <Centered>
              <div className="text-center">
                <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-ok-soft text-ok">
                  <IconCheck size={22} />
                </div>
                <p className="font-medium">Everything looks well filed</p>
                <p className="mt-1 text-sm text-muted">
                  No paper in this scope needs to move.
                </p>
              </div>
            </Centered>
          ) : (
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
                    <div className="min-w-0 flex-[1.4]">
                      <button
                        onClick={() => onOpenItem(row.proposal.itemKey)}
                        className="block w-full truncate text-left text-sm font-medium hover:text-accent"
                        title={titleOf(row.proposal.itemKey)}
                      >
                        {titleOf(row.proposal.itemKey)}
                      </button>
                      <p className="flex flex-wrap gap-1 pt-0.5">
                        {row.proposal.currentPaths.map((p, i) => (
                          <span
                            key={i}
                            className="badge bg-danger-soft text-danger line-through decoration-danger/60"
                            title="Current collection — judged a poor fit"
                          >
                            {p.join(" / ")}
                          </span>
                        ))}
                      </p>
                    </div>
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
          )}
        </div>
      </div>
    );
  }

  // done
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
          {ok.length} {ok.length === 1 ? "paper" : "papers"} refiled
        </h2>
        {failed.length > 0 && (
          <div className="mt-3 space-y-1.5">
            <p className="text-sm text-muted">
              {failed.length} {failed.length === 1 ? "move" : "moves"} failed —
              those papers keep their current filing:
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
            onClose();
          }}
        >
          Back to library
        </button>
      </div>
    </Centered>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-6">{children}</div>
  );
}
