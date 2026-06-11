import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../api";
import type { AppSettings, ProviderId, ZoteroStatus } from "../types";
import {
  IconCheck,
  IconDot,
  IconFolderOpen,
  IconLoader,
  IconX,
} from "../components/icons";

interface Props {
  settings: AppSettings;
  status: ZoteroStatus | null;
  onSave: (next: AppSettings) => Promise<void>;
  onClose: () => void;
}

export default function SettingsView({ settings, status, onSave, onClose }: Props) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saved, setSaved] = useState(false);
  useEffect(() => setDraft(settings), [settings]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);

  const save = async () => {
    await onSave(draft);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl space-y-6 px-6 py-8 pb-24">
        <h1 className="text-xl font-semibold tracking-tight">Settings</h1>

        <ProviderSection draft={draft} setDraft={setDraft} />
        <ZoteroSection draft={draft} setDraft={setDraft} status={status} />
        <FilesSection draft={draft} setDraft={setDraft} />
      </div>

      <div className="sticky bottom-0 border-t border-edge bg-surface/95 px-6 py-3 backdrop-blur">
        <div className="mx-auto flex max-w-2xl items-center gap-2">
          <button className="btn-primary" disabled={!dirty} onClick={save}>
            Save changes
          </button>
          {saved && (
            <span className="badge bg-ok-soft text-ok">
              <IconCheck size={12} /> Saved
            </span>
          )}
          <div className="flex-1" />
          <button className="btn-secondary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="card space-y-4 p-5">
      <div>
        <h2 className="text-sm font-semibold">{title}</h2>
        <p className="mt-0.5 text-xs text-muted">{description}</p>
      </div>
      {children}
    </section>
  );
}

// --- AI Provider -------------------------------------------------------

function ProviderSection({
  draft,
  setDraft,
}: {
  draft: AppSettings;
  setDraft: (s: AppSettings) => void;
}) {
  return (
    <Section
      title="AI Provider"
      description="Used for summaries and classification. Keys are stored in your OS keychain, never in files."
    >
      <div className="grid grid-cols-2 gap-2">
        {(["gemini", "anthropic"] as ProviderId[]).map((p) => (
          <button
            key={p}
            onClick={() => setDraft({ ...draft, defaultProvider: p })}
            className={`rounded-lg border p-3 text-left transition-colors ${
              draft.defaultProvider === p
                ? "border-accent bg-accent-soft"
                : "border-edge bg-raised hover:border-edge-strong"
            }`}
            aria-pressed={draft.defaultProvider === p}
          >
            <p className="text-sm font-semibold">
              {p === "gemini" ? "Google Gemini" : "Anthropic Claude"}
            </p>
            <p className="mt-0.5 text-xs text-muted">
              {p === "gemini" ? "Google AI Studio API" : "Anthropic Messages API"}
            </p>
          </button>
        ))}
      </div>

      <ProviderConfig
        provider="gemini"
        label="Gemini"
        model={draft.geminiModel}
        onModel={(m) => setDraft({ ...draft, geminiModel: m })}
      />
      <ProviderConfig
        provider="anthropic"
        label="Anthropic"
        model={draft.anthropicModel}
        onModel={(m) => setDraft({ ...draft, anthropicModel: m })}
      />
    </Section>
  );
}

function ProviderConfig({
  provider,
  label,
  model,
  onModel,
}: {
  provider: ProviderId;
  label: string;
  model: string;
  onModel: (m: string) => void;
}) {
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState<"save" | "test" | null>(null);
  const [testResult, setTestResult] = useState<"ok" | string | null>(null);

  const refresh = useCallback(() => {
    api.hasApiKey(provider).then(setHasKey).catch(() => setHasKey(null));
  }, [provider]);
  useEffect(refresh, [refresh]);

  const saveKey = async () => {
    setBusy("save");
    setTestResult(null);
    try {
      await api.saveApiKey(provider, keyInput);
      setKeyInput("");
      refresh();
    } catch (e) {
      setTestResult(api.errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const testKey = async () => {
    setBusy("test");
    setTestResult(null);
    try {
      await api.testApiKey(provider);
      setTestResult("ok");
    } catch (e) {
      setTestResult(api.errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const removeKey = async () => {
    await api.deleteApiKey(provider).catch(() => {});
    setTestResult(null);
    refresh();
  };

  return (
    <div className="space-y-2 rounded-lg border border-edge bg-raised p-3">
      <div className="flex items-center gap-2">
        <p className="text-sm font-medium">{label}</p>
        <span
          className={`badge ${hasKey ? "bg-ok-soft text-ok" : "bg-inset text-faint"}`}
          data-testid={`${provider}-key-state`}
        >
          {hasKey === null ? "…" : hasKey ? "Key saved" : "No key"}
        </span>
        <div className="flex-1" />
        <label className="text-xs text-faint" htmlFor={`${provider}-model`}>
          Model
        </label>
        <input
          id={`${provider}-model`}
          className="input h-7 w-44 text-xs"
          value={model}
          onChange={(e) => onModel(e.target.value)}
        />
      </div>
      <div className="flex items-center gap-2">
        <input
          type="password"
          className="input h-8 flex-1"
          placeholder={hasKey ? "Enter a new key to replace the saved one" : "Paste your API key"}
          value={keyInput}
          onChange={(e) => setKeyInput(e.target.value)}
          aria-label={`${label} API key`}
        />
        <button
          className="btn-secondary h-8"
          disabled={!keyInput.trim() || busy !== null}
          onClick={saveKey}
        >
          {busy === "save" ? <IconLoader size={13} /> : null} Save key
        </button>
        <button
          className="btn-secondary h-8"
          disabled={!hasKey || busy !== null}
          onClick={testKey}
        >
          {busy === "test" ? <IconLoader size={13} /> : null} Test
        </button>
        {hasKey && (
          <button className="btn-ghost h-8 text-danger" onClick={removeKey}>
            Remove
          </button>
        )}
      </div>
      {testResult === "ok" && (
        <p className="badge bg-ok-soft text-ok">
          <IconCheck size={12} /> Works
        </p>
      )}
      {testResult && testResult !== "ok" && (
        <p className="rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger">
          {testResult}
        </p>
      )}
    </div>
  );
}

// --- Zotero ------------------------------------------------------------

function ZoteroSection({
  draft,
  setDraft,
  status,
}: {
  draft: AppSettings;
  setDraft: (s: AppSettings) => void;
  status: ZoteroStatus | null;
}) {
  const [exportedTo, setExportedTo] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  const exportXpi = async () => {
    setExportError(null);
    try {
      const dir = await openDialog({ directory: true });
      if (typeof dir !== "string") return;
      setExportedTo(await api.exportPluginXpi(dir));
    } catch (e) {
      setExportError(api.errorMessage(e));
    }
  };

  return (
    <Section
      title="Zotero"
      description="Zotero Notebook talks to the Zotero app running on this computer."
    >
      <div className="flex items-center gap-2 text-sm">
        <span
          className={
            status?.running && status.pluginInstalled
              ? "text-ok"
              : status?.running
                ? "text-warn"
                : "text-danger"
          }
        >
          <IconDot size={8} />
        </span>
        {status?.running && status.pluginInstalled
          ? `Connected — plugin v${status.pluginVersion ?? "?"}`
          : status?.running
            ? "Zotero is running, but the companion plugin is not installed."
            : "Zotero is not running."}
      </div>
      {status?.hint && <p className="text-xs text-muted">{status.hint}</p>}

      <div>
        <label className="text-xs text-faint" htmlFor="zotero-url">
          Zotero server URL
        </label>
        <input
          id="zotero-url"
          className="input mt-1"
          value={draft.zoteroBaseUrl}
          onChange={(e) => setDraft({ ...draft, zoteroBaseUrl: e.target.value })}
        />
        <p className="mt-1 text-xs text-faint">
          Default http://127.0.0.1:23119 — change only for unusual setups.
        </p>
      </div>

      <div className="rounded-lg border border-edge bg-raised p-3">
        <p className="text-sm font-medium">Companion plugin</p>
        <p className="mt-0.5 text-xs text-muted">
          Required for AI classification and synchronized collection/file
          moves. Save the plugin file, then install it inside Zotero.
        </p>
        <button className="btn-secondary mt-2" onClick={exportXpi}>
          <IconFolderOpen size={14} /> Save plugin file (.xpi)…
        </button>
        {exportedTo && (
          <div className="mt-2 space-y-1 text-xs">
            <p className="font-mono text-muted">{exportedTo}</p>
            <ol className="list-inside list-decimal space-y-0.5 text-muted">
              <li>In Zotero: Tools → Plugins</li>
              <li>Gear icon → Install Plugin From File…</li>
              <li>Select the saved .xpi, then restart Zotero</li>
            </ol>
          </div>
        )}
        {exportError && (
          <p className="mt-2 rounded-md bg-danger-soft px-2.5 py-1.5 text-xs text-danger">
            {exportError}
          </p>
        )}
      </div>
    </Section>
  );
}

// --- Files ---------------------------------------------------------------

function FilesSection({
  draft,
  setDraft,
}: {
  draft: AppSettings;
  setDraft: (s: AppSettings) => void;
}) {
  const chooseDir = async () => {
    const dir = await openDialog({ directory: true });
    if (typeof dir === "string") setDraft({ ...draft, fileRoot: dir });
  };

  return (
    <Section
      title="Files"
      description="Where your linked PDF files live (your ZotMoov destination folder)."
    >
      <div className="flex items-center gap-2">
        <button className="btn-secondary shrink-0" onClick={chooseDir}>
          <IconFolderOpen size={14} /> Choose folder…
        </button>
        <input
          className="input flex-1"
          placeholder="Not set — moves only update Zotero collections"
          value={draft.fileRoot ?? ""}
          onChange={(e) =>
            setDraft({ ...draft, fileRoot: e.target.value || null })
          }
          aria-label="Linked files root folder"
        />
        {draft.fileRoot && (
          <button
            className="btn-ghost h-8 w-8 px-0!"
            aria-label="Clear folder"
            onClick={() => setDraft({ ...draft, fileRoot: null })}
          >
            <IconX size={14} />
          </button>
        )}
      </div>
      <p className="text-xs text-faint">
        When set, approved classifications also move the PDF into
        &lt;root&gt;/Collection/Sub-collection/. Leave empty to move only
        Zotero collections and never touch files.
      </p>
    </Section>
  );
}
