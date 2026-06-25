# Zotero Notebook — Architecture

A Windows-first desktop app that mirrors your Zotero library, shows
Zotero-sourced metadata for every PDF, generates AI summaries (Gemini or
Claude), and auto-classifies papers from *Unclassified* into your collection
tree — keeping the Zotero collection and the on-disk folder in sync via a
companion Zotero plugin.

```
┌───────────────────────────────┐        ┌──────────────────────────────┐
│  Desktop app (Tauri 2)        │  HTTP  │  Zotero 7–9 (running)        │
│                               │ 23119  │                              │
│  app/src   React + TS + Vite  │◄──────►│  zotero-plugin (.xpi)        │
│  app/src-tauri  thin commands │        │   /zotero-notebook/ping      │
│  core/     all logic (Rust)   │        │   /zotero-notebook/library   │
│   ├─ zotero  plugin + local   │        │   /zotero-notebook/move-item │
│   ├─ llm     gemini/anthropic │        │  (falls back to read-only    │
│   ├─ db      SQLite sidecar   │        │   /api/users/0 when plugin   │
│   └─ classify, settings, keys │        │   is missing)                │
└───────────────────────────────┘        └──────────────────────────────┘
```

## Design decisions

1. **Zotero is the single source of truth.** The app never keeps its own
   copy of the library; it loads a fresh snapshot on demand. The only local
   state is a sidecar SQLite DB (AI summaries, per-item reading state, cached
   citation graphs, and an AI usage/cost ledger), JSON settings, and API keys
   in the OS keychain.
2. **Writes go through the companion plugin.** Zotero's local API is
   read-only; the plugin executes collection creation / item moves / file
   moves inside Zotero itself, transactionally (see `docs/PLUGIN_API.md`).
   Without the plugin the app runs in read-only mode with a banner.
3. **Classify is review-then-apply.** The LLM only produces proposals; the
   user edits/approves them in the review screen, then the app applies the
   approved moves one by one with per-item progress and per-item failure
   reporting. No background magic.
4. **Write-back is additive and best-effort.** Fetched abstracts fill only
   *empty* Zotero abstract fields, AI tags are added never removed, and the
   summary child note is updated in place (identified by the "AI Summary —
   Zotero Notebook" marker). All write-back goes through the plugin's
   `/update-item`; failures are logged, never block the primary action, and
   both automatic paths (abstracts, summary notes) have Settings toggles.
5. **`core` is headless and fully testable.** Every external surface
   (Zotero, plugin, Gemini, Anthropic) is reached through a configurable
   base URL, so `cargo test -p zn-core` exercises the real client code
   against mock HTTP servers — no Zotero install needed. This is how we test
   despite "the real environment is a Windows machine with Zotero".

## Tauri command surface

Implemented in `app/src-tauri/src/lib.rs`, thin wrappers over `zn_core`.
Errors: every command returns `Result<T, zn_core::Error>` (serialized as a
string). The frontend mirror lives in `app/src/api.ts`.

| Command | Args | Returns | Notes |
|---|---|---|---|
| `get_status` | — | `ZoteroStatus` | Probes plugin ping, then local API ping. Also emitted as `zotero-status` event by a 15 s background watcher. |
| `get_library` | — | `Library` | Plugin `/library`; falls back to local API (read-only, `writable: false`). |
| `get_summary` | `itemKey` | `StoredSummary \| null` | From sidecar DB. |
| `get_all_summaries` | — | `StoredSummary[]` | Whole sidecar table; powers search-over-summaries and the batch button's "N without a summary" count. |
| `get_usage_summary` | — | `UsageSummary` | Cumulative AI token/cost totals from the `usage_log` ledger. Also pushed live as a `usage-update` event after each tracked operation. Cost is an approximate list-price estimate (`core/src/pricing.rs`); the local provider is free; chat is not tracked. |
| `get_reading_states` | — | `ReadingState[]` | The whole `reading_state` sidecar table (status/star/note per item) — powers the status column and the Reading-queue view. |
| `set_reading_state` | `itemKey`, `status: ReadingStatus \| null`, `starred: bool`, `note: string` | `ReadingState \| null` | Upsert the item's reading state. Status / star / note are independent — starring never forces a status (`status` stays `null`). Deletes the row (returns `null`) when status is null, unstarred, and the note is empty (untracked). App-owned local state only — never written back to Zotero. The star powers the Starred view; an explicit to-read/reading status powers the Reading queue. |
| `fetch_citation_graph` | `itemKey`, `refresh?` | `CitationGraph` | References + citing works from OpenAlex (`core/src/citations.rs`), each tagged with library membership (DOI then normalized-title match). Read-only / suggest-only — no Zotero writes. Cached in the `citation_cache` sidecar table (14-day TTL); `refresh: true` re-fetches. `fetchFailed` is set when the item has no DOI or OpenAlex was unreachable. |
| `summarize_items` | `itemKeys: string[]`, `provider?` | `StoredSummary[]` | Batch quick-summarize (metadata+abstract only); sequential, emits `summarize-progress`, per-item failures don't abort. |
| `save_summary_note` | `itemKey` | — | Manual "Save to Zotero": pushes the stored summary as a child note via plugin `/update-item` (upserted in place by marker). |
| `summarize_item` | `itemKey`, `provider?`, `useFulltext?` | `StoredSummary` | Default: metadata+abstract prompt (cheap). `useFulltext: true` (a separate UI button) additionally sends up to 80k chars of the PDF's extracted text via the plugin. The result records its `source` (fulltext/abstract/metadata) for the UI badge. |
| `chat_with_item` | `itemKey`, `history: ChatMessage[]`, `provider?` | `string` | Per-paper "Ask AI" chat. Context = metadata + extracted PDF text (80k cap, plugin `/fulltext`). Streams fragments as `chat-delta` events (`ChatDelta`), resolves with the full answer. Answers always in English. |
| `chat_with_items` | `itemKeys: string[]`, `history: ChatMessage[]`, `provider?` | `string` | Multi-paper synthesis / Q&A over a set of items (a whole collection or an ad-hoc selection). Context = metadata + abstracts only (no PDF text), capped at `MAX_SYNTHESIS_PAPERS` (50), each abstract truncated; built by `synthesis::build_context`. Streams fragments as `synthesis-delta` events (`SynthesisDelta`, no item key), resolves with the full answer. Answers always in English. |
| `classify_items` | `itemKeys: string[]`, `provider?` | `ClassificationProposal[]` | Sequential; emits `classify-progress` (`ProgressEvent`) per item. |
| `audit_items` | `itemKeys: string[]`, `provider?` | `AuditProposal[]` | Re-checks already-classified papers ("is the current filing right?"); conservative prompt — flags only when no current collection fits. Emits `audit-progress`. |
| `apply_classifications` | `decisions: ClassificationDecision[]` | `MoveResult[]` | Plugin move-item per decision; emits `apply-progress`. Continues past per-item failures. Removes the Unclassified membership plus any `removeCollectionKeys` on the decision (audit flow). |
| `reveal_item_file` | `itemKey` | — | Opens OS file manager with the file selected (`explorer /select,` on Windows, `open -R` on macOS, `xdg-open` parent dir on Linux). |
| `open_item_pdf` | `itemKey` | — | Opens the PDF with the default app. |
| `open_in_zotero` | `itemKey` | — | Opens `zotero://select/library/items/<KEY>`. |
| `get_settings` / `save_settings` | — / `AppSettings` | `AppSettings` / — | JSON file in the app config dir. |
| `save_api_key` | `provider`, `key` | — | OS keychain, service `zotero-notebook`. |
| `has_api_key` / `delete_api_key` | `provider` | `bool` / — | |
| `test_api_key` | `provider` | — | Cheap live request; error message explains failure. |
| `export_plugin_xpi` | `destDir` | `string` | Writes the bundled `.xpi` (Tauri resource) into `destDir`, returns the full path. Used by Settings → "Install Zotero plugin". |

Events: `zotero-status` (`ZoteroStatus`), `classify-progress`,
`audit-progress`, `apply-progress`, `summarize-progress` (all
`ProgressEvent`), `chat-delta` (`ChatDelta`, streamed per-paper chat
fragments), `synthesis-delta` (`SynthesisDelta`, streamed multi-paper
synthesis/Q&A fragments), `usage-update` (`UsageSummary`, cumulative
token/cost totals after each tracked AI operation).

Token/cost tracking captures real usage from each provider's non-streaming
response via an interior-mutable side-channel on the client (`AnyProvider::
last_usage()`) — no method signatures change. The commands log a `usage_log`
row (op/provider/model/tokens/cost) and emit `usage-update`. Streaming chat is
not yet metered.

## LLM providers

`core/src/llm/provider.rs` defines the shared request/response types and
`AnyProvider` (enum dispatch over `GeminiClient` / `AnthropicClient` /
`OpenAiCompatClient` — no trait objects, no async_trait). The third client
covers local OpenAI-compatible runtimes (Ollama `/v1`, LM Studio,
llama.cpp, vLLM): no API key required (optional Bearer), structured output
requested via `response_format: json_schema` with the schema also embedded
in the prompt, a one-shot retry without `response_format` for servers that
reject the parameter, and a tolerant JSON extractor for fenced/prosy
replies. Configured via `AppSettings.local_base_url` / `local_model`.

- **Summarize**: metadata-only prompt (title, authors, venue, year,
  abstract). 5–8 sentence English summary. PDFs are never uploaded.
  Missing abstracts are backfilled best-effort from Crossref → Semantic
  Scholar → OpenAlex (`core/src/abstract_lookup.rs`, key-less public APIs)
  before any summarize/classify/audit call; summaries generated with no
  abstract at all are stored with `had_abstract = false` and badged in the
  UI as title/venue-only.
- **Classify**: prompt contains the item metadata **and** the existing
  collection tree (as `A / B / C` paths). The model must prefer an existing
  path and may propose a new one only when nothing fits; structured JSON
  output (`{path: string[], isNew, confidence, rationale}`) enforced via
  Gemini `responseSchema` / Anthropic `output_config.format` json_schema.
- Defaults: Gemini `gemini-2.5-pro`, Anthropic `claude-opus-4-8` (both
  user-configurable in Settings). Anthropic uses raw Messages API
  (`POST /v1/messages`, headers `x-api-key`, `anthropic-version:
  2023-06-01`); **no `temperature`/`top_p`** (removed on Opus 4.7+).
  Key test: minimal 1-token request; Gemini key test uses
  `gemini-2.5-flash` `:generateContent`.

## Frontend (app/src)

Vite + React 18 + TypeScript + Tailwind v4. No router — a single-window
desktop shell with view state in `App.tsx`. Design tokens (light/dark) live
in `src/styles.css`; components use them via Tailwind utilities and CSS
variables. Search is client-side Fuse.js over the loaded library plus cached
summaries, opened with `Ctrl/Cmd+K`.

Views: Library (sidebar tree + item table), Item detail (modal), Unclassified
(list + "Classify with AI" → review table → apply with progress), Reading
queue (cross-collection list of to-read/reading items), Starred
(cross-collection list of starred items, decoupled from reading status),
Settings (providers/keys/models/file root/plugin install), Onboarding (first
run).

New-import detection is purely client-side: `App.tsx` diffs the Unclassified
key set across library refreshes (refresh button / window-focus, throttled)
and shows a dismissible banner that routes into the Unclassified review flow —
no backend command, no auto-filing.

## Building

- Dev (needs system WebKit/GTK on Linux): `cd app && npm run tauri dev`
- Tests: `cargo test -p zn-core` · `cd app && npm test && npx tsc --noEmit`
- Windows installer: built by `.github/workflows/build.yml` (NSIS `.exe`),
  along with the plugin `.xpi`. The `.xpi` is also bundled into the app as a
  resource so Settings can export it for installation into Zotero.
