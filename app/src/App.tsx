// Application shell: owns global state (status, library, settings, selection)
// and wires the views together. Leaf components live in ./components and
// ./views — their prop contracts are defined here.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "./api";
import type {
  AppSettings,
  Item,
  Library,
  ReadingState,
  StoredSummary,
  UsageSummary,
  ZoteroStatus,
} from "./types";
import { queueItems, unclassifiedItems } from "./lib/library";
import Sidebar from "./components/Sidebar";
import Topbar from "./components/Topbar";
import SearchPalette from "./components/SearchPalette";
import ItemDetailModal from "./components/ItemDetailModal";
import NewImportsBanner from "./components/NewImportsBanner";
import LibraryView from "./views/LibraryView";
import UnclassifiedView from "./views/UnclassifiedView";
import SettingsView from "./views/SettingsView";
import OnboardingView from "./views/OnboardingView";

/** Sidebar selection: a collection key or one of the special views. */
export type Selection =
  | { kind: "all" }
  | { kind: "unclassified" }
  | { kind: "queue" }
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
  const [readingStates, setReadingStates] = useState<Map<string, ReadingState>>(
    () => new Map(),
  );
  // New-import detection: keys flagged as newly-appeared in Unclassified, and
  // the previous Unclassified key set to diff against (null until first load).
  const [newImportKeys, setNewImportKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const prevUnclassifiedRef = useRef<Set<string> | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);

  const refreshSummaries = useCallback(() => {
    api.getAllSummaries().then(setSummaries).catch(() => {});
  }, []);

  const refreshReadingStates = useCallback(() => {
    api
      .getReadingStates()
      .then((list) => setReadingStates(new Map(list.map((s) => [s.itemKey, s]))))
      .catch(() => {});
  }, []);

  // Optimistically reflect a single edit from the detail modal without a round
  // trip to reload the whole queue.
  const applyReadingState = useCallback(
    (itemKey: string, next: ReadingState | null) => {
      setReadingStates((prev) => {
        const m = new Map(prev);
        if (next) m.set(itemKey, next);
        else m.delete(itemKey);
        return m;
      });
    },
    [],
  );

  const refreshLibrary = useCallback(async () => {
    setRefreshing(true);
    refreshSummaries();
    refreshReadingStates();
    try {
      const lib = await api.getLibrary();
      setLibrary(lib);
      setLibraryError(null);
      // Detect newly-imported unclassified papers by diffing against the
      // previous snapshot. Skip an empty fetch (a transient blip) so a later
      // repopulate doesn't flag the whole library as "new".
      if (lib.items.length > 0) {
        const current = new Set(unclassifiedItems(lib).map((i) => i.key));
        const prev = prevUnclassifiedRef.current;
        if (prev) {
          const fresh = [...current].filter((k) => !prev.has(k));
          if (fresh.length > 0) {
            setNewImportKeys((s) => {
              const next = new Set(s);
              fresh.forEach((k) => next.add(k));
              return next;
            });
          }
        }
        prevUnclassifiedRef.current = current;
      }
    } catch (e) {
      setLibraryError(api.errorMessage(e));
    } finally {
      setRefreshing(false);
    }
  }, [refreshSummaries, refreshReadingStates]);

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
    void api.getUsageSummary().then(setUsage).catch(() => {});
    const un = api.onZoteroStatus((s) => setStatus(s));
    const unUsage = api.onUsageUpdate(setUsage);
    return () => {
      void un.then((f) => f());
      void unUsage.then((f) => f());
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

  // Refresh when the window regains focus (e.g. after importing in Zotero) so
  // new-import detection feels automatic. Throttled to avoid thrashing.
  useEffect(() => {
    let last = Date.now();
    const onFocus = () => {
      if (Date.now() - last > 20_000) {
        last = Date.now();
        void refreshLibrary();
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshLibrary]);

  const unclassified = useMemo(() => unclassifiedItems(library), [library]);
  const queue = useMemo(
    () => queueItems(library, readingStates),
    [library, readingStates],
  );
  const newImports = useMemo(
    () => unclassified.filter((i) => newImportKeys.has(i.key)),
    [unclassified, newImportKeys],
  );
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
        queueCount={queue.length}
        onSelect={(sel) => {
          setSelection(sel);
          setView("library");
        }}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <Topbar
          status={status}
          usage={usage}
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
        {newImports.length > 0 &&
          view !== "settings" &&
          selection.kind !== "unclassified" && (
            <NewImportsBanner
              count={newImports.length}
              onClassify={() => {
                setSelection({ kind: "unclassified" });
                setView("library");
                setNewImportKeys(new Set());
              }}
              onDismiss={() => setNewImportKeys(new Set())}
            />
          )}
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
              readingStates={readingStates}
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
          readingState={readingStates.get(openItem.key) ?? null}
          onReadingChanged={(next) => applyReadingState(openItem.key, next)}
          onOpenItem={setOpenItemKey}
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
