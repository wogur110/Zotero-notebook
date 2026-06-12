// Application shell: owns global state (status, library, settings, selection)
// and wires the views together. Leaf components live in ./components and
// ./views — their prop contracts are defined here.

import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "./api";
import type {
  AppSettings,
  Item,
  Library,
  StoredSummary,
  ZoteroStatus,
} from "./types";
import { unclassifiedItems } from "./lib/library";
import Sidebar from "./components/Sidebar";
import Topbar from "./components/Topbar";
import SearchPalette from "./components/SearchPalette";
import ItemDetailModal from "./components/ItemDetailModal";
import LibraryView from "./views/LibraryView";
import UnclassifiedView from "./views/UnclassifiedView";
import SettingsView from "./views/SettingsView";
import OnboardingView from "./views/OnboardingView";

/** Sidebar selection: a collection key or one of the special views. */
export type Selection =
  | { kind: "all" }
  | { kind: "unclassified" }
  | { kind: "collection"; key: string };

export type MainView = "library" | "settings";

const EMPTY_LIBRARY: Library = { collections: [], items: [], writable: false };

export default function App() {
  const [status, setStatus] = useState<ZoteroStatus | null>(null);
  const [library, setLibrary] = useState<Library>(EMPTY_LIBRARY);
  const [libraryError, setLibraryError] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [selection, setSelection] = useState<Selection>({ kind: "all" });
  const [view, setView] = useState<MainView>("library");
  const [openItemKey, setOpenItemKey] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [onboarded, setOnboarded] = useState(
    () => localStorage.getItem("zn-onboarded") === "1",
  );
  const [summaries, setSummaries] = useState<StoredSummary[]>([]);

  const refreshSummaries = useCallback(() => {
    api.getAllSummaries().then(setSummaries).catch(() => {});
  }, []);

  const refreshLibrary = useCallback(async () => {
    setRefreshing(true);
    refreshSummaries();
    try {
      setLibrary(await api.getLibrary());
      setLibraryError(null);
    } catch (e) {
      setLibraryError(api.errorMessage(e));
    } finally {
      setRefreshing(false);
    }
  }, [refreshSummaries]);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.getStatus());
    } catch {
      /* status probe never throws user-visible errors */
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    void refreshLibrary();
    void api.getSettings().then(setSettings).catch(() => {});
    const un = api.onZoteroStatus((s) => setStatus(s));
    return () => {
      void un.then((f) => f());
    };
  }, [refreshStatus, refreshLibrary]);

  // A refresh can remove the selected collection (deleted in Zotero) or the
  // open item — fall back instead of rendering a stale/empty view.
  useEffect(() => {
    if (
      selection.kind === "collection" &&
      library.items.length + library.collections.length > 0 &&
      !library.collections.some((c) => c.key === selection.key)
    ) {
      setSelection({ kind: "all" });
    }
  }, [library, selection]);

  // Global shortcuts: Ctrl/Cmd+K search, Esc closes overlays.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearchOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const unclassified = useMemo(() => unclassifiedItems(library), [library]);
  const summarizedKeys = useMemo(
    () => new Set(summaries.map((s) => s.itemKey)),
    [summaries],
  );
  const openItem: Item | null = useMemo(
    () => library.items.find((i) => i.key === openItemKey) ?? null,
    [library, openItemKey],
  );

  const handleSaveSettings = useCallback(async (next: AppSettings) => {
    await api.saveSettings(next);
    setSettings(next);
  }, []);

  if (!onboarded) {
    return (
      <OnboardingView
        status={status}
        onRefreshStatus={refreshStatus}
        onDone={() => {
          localStorage.setItem("zn-onboarded", "1");
          setOnboarded(true);
          void refreshLibrary();
        }}
      />
    );
  }

  return (
    <div className="flex h-full">
      <Sidebar
        library={library}
        selection={selection}
        unclassifiedCount={unclassified.length}
        onSelect={(sel) => {
          setSelection(sel);
          setView("library");
        }}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <Topbar
          status={status}
          refreshing={refreshing}
          view={view}
          onRefresh={() => {
            void refreshStatus();
            void refreshLibrary();
          }}
          onOpenSearch={() => setSearchOpen(true)}
          onToggleSettings={() =>
            setView((v) => (v === "settings" ? "library" : "settings"))
          }
        />
        <main className="min-h-0 flex-1 overflow-hidden">
          {view === "settings" ? (
            settings && (
              <SettingsView
                settings={settings}
                status={status}
                onSave={handleSaveSettings}
                onClose={() => setView("library")}
              />
            )
          ) : selection.kind === "unclassified" ? (
            <UnclassifiedView
              library={library}
              items={unclassified}
              writable={library.writable}
              defaultProvider={settings?.defaultProvider ?? "gemini"}
              onOpenItem={setOpenItemKey}
              onApplied={refreshLibrary}
            />
          ) : (
            <LibraryView
              library={library}
              selection={selection}
              error={libraryError}
              defaultProvider={settings?.defaultProvider ?? "gemini"}
              summarizedKeys={summarizedKeys}
              onOpenItem={setOpenItemKey}
              onRetry={refreshLibrary}
              onApplied={refreshLibrary}
              onSummarized={refreshSummaries}
            />
          )}
        </main>
      </div>

      {openItem && (
        <ItemDetailModal
          key={openItem.key} // remount per item: no stale summary/error state
          item={openItem}
          library={library}
          defaultProvider={settings?.defaultProvider ?? "gemini"}
          onClose={() => {
            setOpenItemKey(null);
            refreshSummaries(); // a summary may have been (re)generated
          }}
        />
      )}
      <SearchPalette
        open={searchOpen}
        library={library}
        summaries={summaries}
        onClose={() => setSearchOpen(false)}
        onOpenItem={(key) => {
          setSearchOpen(false);
          setOpenItemKey(key);
        }}
      />
    </div>
  );
}
