// Batch quick-summarize for every paper in the current view that has no
// summary yet. Confirm → sequential progress → results. Quick mode only
// (metadata + abstract): cheap by design — full-text summaries stay a
// deliberate per-paper action in the detail popup.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { Item, ProgressEvent, ProviderId } from "../types";
import ProgressCard from "../components/ProgressCard";
import { IconAlert, IconCheck, IconSparkles } from "../components/icons";

interface Props {
  /** Items in the current view that have no summary yet. */
  items: Item[];
  scopeLabel: string;
  defaultProvider: ProviderId;
  onClose: () => void;
  /** Called when at least one summary was created. */
  onSummarized: () => void;
}

type Phase = "confirm" | "running" | "done";

interface FailedRow {
  itemKey: string;
  message: string;
}

export default function SummarizeFlow({
  items,
  scopeLabel,
  defaultProvider,
  onClose,
  onSummarized,
}: Props) {
  const [phase, setPhase] = useState<Phase>("confirm");
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [okCount, setOkCount] = useState(0);
  const [failures, setFailures] = useState<FailedRow[]>([]);
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
    items.find((i) => i.key === key)?.title ?? key;

  const start = async () => {
    const runId = ++runIdRef.current;
    setPhase("running");
    setFatal(null);
    setProgress({ done: 0, total: items.length, itemKey: null, state: "running", message: null });
    const errors: FailedRow[] = [];
    const un = await api.onSummarizeProgress((p) => {
      if (runIdRef.current !== runId) return;
      setProgress(p);
      if (p.state === "error" && p.itemKey) {
        errors.push({ itemKey: p.itemKey, message: p.message ?? "failed" });
      }
    });
    unlistenRef.current.push(un);
    try {
      const created = await api.summarizeItems(
        items.map((i) => i.key),
        defaultProvider,
      );
      if (runIdRef.current !== runId) return;
      setOkCount(created.length);
      setFailures(errors);
      if (created.length > 0) onSummarized();
      setPhase("done");
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

  if (phase === "confirm") {
    return (
      <Centered>
        <div className="card w-[480px] max-w-[90vw] p-6">
          <div className="mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full bg-accent-soft text-accent">
            <IconSparkles size={20} />
          </div>
          <h2 className="text-center text-lg font-semibold">
            Summarize {items.length} {items.length === 1 ? "paper" : "papers"}?
          </h2>
          <p className="mt-2 text-center text-sm text-muted">
            Scope: <span className="font-medium text-text">{scopeLabel}</span> —
            every paper there without a summary. Quick mode (metadata +
            abstract), one AI request per paper ({items.length} total).
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
            <button className="btn-primary" onClick={() => void start()}>
              <IconSparkles size={14} /> Start summarizing
            </button>
          </div>
        </div>
      </Centered>
    );
  }

  if (phase === "running") {
    const pct =
      progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
    return (
      <ProgressCard
        title={`Summarizing paper ${Math.min((progress?.done ?? 0) + 1, progress?.total ?? 1)} of ${progress?.total ?? items.length}`}
        subtitle={progress?.itemKey ? titleOf(progress.itemKey) : "…"}
        pct={pct}
      />
    );
  }

  return (
    <Centered>
      <div className="card w-[480px] max-w-[90vw] p-6">
        <div
          className={`mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full ${
            failures.length === 0 ? "bg-ok-soft text-ok" : "bg-warn-soft text-warn"
          }`}
        >
          {failures.length === 0 ? <IconCheck size={20} /> : <IconAlert size={20} />}
        </div>
        <h2 className="text-center text-lg font-semibold">
          {okCount} {okCount === 1 ? "summary" : "summaries"} created
        </h2>
        {failures.length > 0 && (
          <div className="mt-3 space-y-1.5">
            <p className="text-sm text-muted">
              {failures.length} {failures.length === 1 ? "paper" : "papers"}{" "}
              failed — you can retry them later:
            </p>
            <ul className="max-h-40 space-y-1 overflow-y-auto">
              {failures.map((f) => (
                <li
                  key={f.itemKey}
                  className="rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger"
                >
                  <span className="font-medium">{titleOf(f.itemKey)}</span> —{" "}
                  {f.message}
                </li>
              ))}
            </ul>
          </div>
        )}
        <button className="btn-primary mt-5 w-full" onClick={onClose}>
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
