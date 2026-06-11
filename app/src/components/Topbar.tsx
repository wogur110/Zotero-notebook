import type { MainView } from "../App";
import type { ZoteroStatus } from "../types";
import { IconDot, IconGear, IconRefresh, IconSearch } from "./icons";

interface Props {
  status: ZoteroStatus | null;
  refreshing: boolean;
  view: MainView;
  onRefresh: () => void;
  onOpenSearch: () => void;
  onToggleSettings: () => void;
}

export default function Topbar({
  status,
  refreshing,
  view,
  onRefresh,
  onOpenSearch,
  onToggleSettings,
}: Props) {
  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-edge bg-surface px-4">
      <button
        onClick={onOpenSearch}
        className="flex h-8 flex-1 max-w-md items-center gap-2 rounded-md border border-edge bg-raised px-3 text-sm text-faint transition-colors hover:border-edge-strong"
        aria-label="Search papers"
      >
        <IconSearch size={14} />
        <span className="flex-1 text-left">Search papers…</span>
        <kbd className="kbd">Ctrl K</kbd>
      </button>

      <div className="flex-1" />

      <StatusPill status={status} />

      <button
        onClick={onRefresh}
        aria-label="Refresh library"
        title="Refresh library"
        className="btn-ghost h-8 w-8 px-0!"
      >
        <IconRefresh size={15} className={refreshing ? "animate-spin" : ""} />
      </button>
      <button
        onClick={onToggleSettings}
        aria-label="Settings"
        title="Settings"
        className={`btn-ghost h-8 w-8 px-0! ${
          view === "settings" ? "bg-inset text-text!" : ""
        }`}
      >
        <IconGear size={15} />
      </button>
    </header>
  );
}

function StatusPill({ status }: { status: ZoteroStatus | null }) {
  if (!status) {
    return (
      <span className="badge bg-inset text-faint">
        <IconDot size={7} /> Connecting…
      </span>
    );
  }
  if (status.running && status.pluginInstalled) {
    return (
      <span className="badge bg-ok-soft text-ok" title="Connected to Zotero">
        <IconDot size={7} />
        Zotero{status.pluginVersion ? ` · plugin v${status.pluginVersion}` : ""}
      </span>
    );
  }
  if (status.running) {
    return (
      <span
        className="badge bg-warn-soft text-warn"
        title={status.hint ?? "The companion plugin is not installed."}
      >
        <IconDot size={7} />
        Zotero (read-only)
      </span>
    );
  }
  return (
    <span
      className="badge bg-danger-soft text-danger"
      title={status.hint ?? "Zotero is not running."}
    >
      <IconDot size={7} />
      Zotero offline
    </span>
  );
}
