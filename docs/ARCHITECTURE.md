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
   state is a sidecar SQLite DB (AI summaries, keyed by Zotero item key),
   JSON settings, and API keys in the OS keychain.
2. **Writes go through the companion plugin.** Zotero's local API is
   read-only; the plugin executes collection creation / item moves / file
   moves inside Zotero itself, transactionally (see `docs/PLUGIN_API.md`).
   Without the plugin the app runs in read-only mode with a banner.
3. **Classify is review-then-apply.** The LLM only produces proposals; the
   user edits/approves them in the review screen, then the app applies the
   approved moves one by one with per-item progress and per-item failure
   reporting. No background magic.
4. **`core` is headless and fully testable.** Every external surface
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
| `summarize_item` | `itemKey`, `provider?`, `useFulltext?` | `StoredSummary` | Default: metadata+abstract prompt (cheap). `useFulltext: true` (a separate UI button) additionally sends up to 80k chars of the PDF's extracted text via the plugin. The result records its `source` (fulltext/abstract/metadata) for the UI badge. |
| `chat_with_item` | `itemKey`, `history: ChatMessage[]`, `provider?` | `string` | Per-paper "Ask AI" chat. Context = metadata + extracted PDF text (80k cap, plugin `/fulltext`). Streams fragments as `chat-delta` events (`ChatDelta`), resolves with the full answer. Answers always in English. |
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
`audit-progress`, `apply-progress` (all `ProgressEvent`), `chat-delta`
(`ChatDelta`, streamed chat fragments).

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
(list + "Classify with AI" → review table → apply with progress), Settings
(providers/keys/models/file root/plugin install), Onboarding (first run).

## Building

- Dev (needs system WebKit/GTK on Linux): `cd app && npm run tauri dev`
- Tests: `cargo test -p zn-core` · `cd app && npm test && npx tsc --noEmit`
- Windows installer: built by `.github/workflows/build.yml` (NSIS `.exe`),
  along with the plugin `.xpi`. The `.xpi` is also bundled into the app as a
  resource so Settings can export it for installation into Zotero.
