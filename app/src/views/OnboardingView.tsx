import { useEffect, useState } from "react";
import * as api from "../api";
import type { ZoteroStatus } from "../types";
import {
  IconAlert,
  IconCheck,
  IconLibrary,
  IconSparkles,
} from "../components/icons";

interface Props {
  status: ZoteroStatus | null;
  onRefreshStatus: () => void;
  onDone: () => void;
}

export default function OnboardingView({ status, onRefreshStatus, onDone }: Props) {
  const [hasAnyKey, setHasAnyKey] = useState<boolean | null>(null);

  useEffect(() => {
    Promise.all([api.hasApiKey("gemini"), api.hasApiKey("anthropic")])
      .then(([g, a]) => setHasAnyKey(g || a))
      .catch(() => setHasAnyKey(false));
  }, []);

  return (
    <div className="h-full overflow-y-auto bg-bg">
      <div className="mx-auto max-w-lg space-y-6 px-6 py-16">
        <div className="text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-xl bg-accent text-accent-text">
            <IconLibrary size={26} />
          </div>
          <h1 className="text-2xl font-semibold tracking-tight">
            Zotero Notebook
          </h1>
          <p className="mt-2 text-sm text-muted">
            Your Zotero library with AI summaries and automatic classification.
          </p>
        </div>

        <div className="space-y-3">
          <ChecklistCard
            ok={status?.running ?? false}
            title="Zotero is running"
            okText="Connected to Zotero on this computer."
            pendingText="Start Zotero, then retry."
            action={
              !status?.running && (
                <button className="btn-ghost text-xs" onClick={onRefreshStatus}>
                  Retry
                </button>
              )
            }
          />
          <ChecklistCard
            ok={status?.pluginInstalled ?? false}
            neutral={!status?.pluginInstalled}
            title="Companion plugin installed"
            okText={`Plugin v${status?.pluginVersion ?? "?"} detected.`}
            pendingText="Install it later from Settings — required for AI classification moves."
          />
          <ChecklistCard
            ok={hasAnyKey ?? false}
            neutral={!hasAnyKey}
            title="AI provider key"
            okText="An API key is configured."
            pendingText="Optional now — add a Gemini or Anthropic key in Settings to enable summaries and classification."
            icon={<IconSparkles size={16} />}
          />
        </div>

        <div className="text-center">
          <button className="btn-primary px-6" onClick={onDone}>
            Open my library
          </button>
          <p className="mt-2 text-xs text-faint">
            You can finish any step later from Settings.
          </p>
        </div>
      </div>
    </div>
  );
}

function ChecklistCard({
  ok,
  neutral = false,
  title,
  okText,
  pendingText,
  action,
  icon,
}: {
  ok: boolean;
  neutral?: boolean;
  title: string;
  okText: string;
  pendingText: string;
  action?: React.ReactNode;
  icon?: React.ReactNode;
}) {
  return (
    <div className="card flex items-center gap-3 p-4">
      <span
        className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full ${
          ok
            ? "bg-ok-soft text-ok"
            : neutral
              ? "bg-inset text-faint"
              : "bg-danger-soft text-danger"
        }`}
      >
        {ok ? <IconCheck size={15} /> : (icon ?? <IconAlert size={15} />)}
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-xs text-muted">{ok ? okText : pendingText}</p>
      </div>
      {action}
    </div>
  );
}
