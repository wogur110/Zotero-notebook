// Single point of contact with the Tauri backend. Components never call
// invoke() directly — they use these wrappers, which keeps mocking trivial.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  AuditProposal,
  ChatDelta,
  ChatMessage,
  ClassificationDecision,
  ClassificationProposal,
  Library,
  MoveResult,
  ProgressEvent,
  ProviderId,
  StoredSummary,
  ZoteroStatus,
} from "./types";

export const getStatus = () => invoke<ZoteroStatus>("get_status");
export const getLibrary = () => invoke<Library>("get_library");

export const getSummary = (itemKey: string) =>
  invoke<StoredSummary | null>("get_summary", { itemKey });

export const getAllSummaries = () =>
  invoke<StoredSummary[]>("get_all_summaries");

/** Batch quick-summarize; progress streams via `onSummarizeProgress`. */
export const summarizeItems = (itemKeys: string[], provider?: ProviderId) =>
  invoke<StoredSummary[]>("summarize_items", {
    itemKeys,
    provider: provider ?? null,
  });

export const summarizeItem = (
  itemKey: string,
  provider?: ProviderId,
  useFulltext = false,
) =>
  invoke<StoredSummary>("summarize_item", {
    itemKey,
    provider: provider ?? null,
    useFulltext,
  });

/** One chat turn; the answer also streams via `onChatDelta`. */
export const chatWithItem = (
  itemKey: string,
  history: ChatMessage[],
  provider?: ProviderId,
) =>
  invoke<string>("chat_with_item", {
    itemKey,
    history,
    provider: provider ?? null,
  });

export const classifyItems = (itemKeys: string[], provider?: ProviderId) =>
  invoke<ClassificationProposal[]>("classify_items", {
    itemKeys,
    provider: provider ?? null,
  });

export const auditItems = (itemKeys: string[], provider?: ProviderId) =>
  invoke<AuditProposal[]>("audit_items", {
    itemKeys,
    provider: provider ?? null,
  });

export const applyClassifications = (decisions: ClassificationDecision[]) =>
  invoke<MoveResult[]>("apply_classifications", { decisions });

export const revealItemFile = (itemKey: string) =>
  invoke<void>("reveal_item_file", { itemKey });

export const openItemPdf = (itemKey: string) =>
  invoke<void>("open_item_pdf", { itemKey });

export const openInZotero = (itemKey: string) =>
  invoke<void>("open_in_zotero", { itemKey });

export const getSettings = () => invoke<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) =>
  invoke<void>("save_settings", { settings });

export const saveApiKey = (provider: ProviderId, key: string) =>
  invoke<void>("save_api_key", { provider, key });
export const hasApiKey = (provider: ProviderId) =>
  invoke<boolean>("has_api_key", { provider });
export const deleteApiKey = (provider: ProviderId) =>
  invoke<void>("delete_api_key", { provider });
export const testApiKey = (provider: ProviderId) =>
  invoke<void>("test_api_key", { provider });

export const exportPluginXpi = (destDir: string) =>
  invoke<string>("export_plugin_xpi", { destDir });

// Events --------------------------------------------------------------

export const onZoteroStatus = (cb: (s: ZoteroStatus) => void): Promise<UnlistenFn> =>
  listen<ZoteroStatus>("zotero-status", (e) => cb(e.payload));

export const onClassifyProgress = (
  cb: (p: ProgressEvent) => void,
): Promise<UnlistenFn> =>
  listen<ProgressEvent>("classify-progress", (e) => cb(e.payload));

export const onSummarizeProgress = (
  cb: (p: ProgressEvent) => void,
): Promise<UnlistenFn> =>
  listen<ProgressEvent>("summarize-progress", (e) => cb(e.payload));

export const onChatDelta = (
  cb: (d: ChatDelta) => void,
): Promise<UnlistenFn> => listen<ChatDelta>("chat-delta", (e) => cb(e.payload));

export const onAuditProgress = (
  cb: (p: ProgressEvent) => void,
): Promise<UnlistenFn> =>
  listen<ProgressEvent>("audit-progress", (e) => cb(e.payload));

export const onApplyProgress = (
  cb: (p: ProgressEvent) => void,
): Promise<UnlistenFn> =>
  listen<ProgressEvent>("apply-progress", (e) => cb(e.payload));

/** Normalize an unknown thrown value (Tauri serializes errors to strings). */
export const errorMessage = (e: unknown): string =>
  typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
