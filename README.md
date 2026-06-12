# Zotero Notebook

A desktop companion for [Zotero](https://www.zotero.org/) that mirrors your
library, shows Zotero-sourced metadata for every PDF, writes AI summaries
(Google Gemini **or** Anthropic Claude), and auto-classifies papers from
*Unclassified* into your collection tree — moving the Zotero collection and
the PDF's folder on disk **together**, transactionally.

```
┌───────────────────────────────┐        ┌──────────────────────────────┐
│  Zotero Notebook (Tauri 2)    │  HTTP  │  Zotero 7–9 (running)        │
│                               │ 23119  │                              │
│  React UI  ·  Rust core       │◄──────►│  companion plugin (.xpi)     │
│  AI: Gemini / Claude          │        │   /zotero-notebook/ping      │
│  SQLite sidecar (summaries)   │        │   /zotero-notebook/library   │
│  OS keychain (API keys)       │        │   /zotero-notebook/move-item │
└───────────────────────────────┘        └──────────────────────────────┘
```

Zotero stays the single source of truth — the app never keeps its own copy
of your library. Writes (creating collections, moving items, moving files)
run **inside Zotero** through the bundled companion plugin, with rollback on
failure.

## Features

- **Live mirror of your Zotero collections** — the full nested tree, with
  item counts, straight from the running Zotero instance.
- **Zotero metadata for every PDF** — authors, venue, year, DOI, tags,
  abstract, collections, and the resolved file path on disk.
- **AI summaries with Gemini, Claude, *or* a local LLM** — one click in the
  paper popup; stored locally in SQLite and keyed to the Zotero item. The
  default summary uses metadata + abstract (cheap); a separate **Full-text
  summary** button reads the whole PDF (via Zotero's extracted text) for a
  deeper summary. Models are configurable (defaults: `gemini-2.5-pro`,
  `claude-opus-4-8`); cloud keys live in the OS keychain. The local
  provider talks to any OpenAI-compatible server (Ollama, LM Studio,
  llama.cpp) — see [Using a local LLM](#using-a-local-llm-no-cloud-no-api-key).
- **Ask AI about a paper** — a chat tab in the paper popup, grounded in the
  PDF's extracted full text, with streaming answers (always in English).
- **Fast search** — `Ctrl/Cmd+K` fuzzy search across titles, authors, tags,
  venues, abstracts, **and your stored AI summaries**.
- **Batch summarize** — a **Summarize N** button in the library header
  quick-summarizes every paper in the current view that has no summary yet
  (confirm first, per-paper progress, per-paper failure reporting).
- **Paper popup** — full metadata, AI summary, **Open PDF**, **Show in
  Folder** (Explorer/Finder with the file selected), and **Open in Zotero**.
- **Review-then-apply AI classification** — for everything in
  *Unclassified*: the AI proposes a collection per paper (preferring your
  existing collections, proposing a new one only when nothing fits), you
  edit/approve in a review table, then the app applies the moves with
  per-paper progress. Each move updates the Zotero collection **and**
  relocates the linked PDF to the matching folder, atomically with rollback.
- **Filing check for classified papers** — the **Check filing** button (on
  All Papers or any collection) asks the AI to re-examine each paper's
  current collection. It is deliberately conservative: a paper is flagged
  only when *no* current collection fits, and you review every proposed
  move (current → suggested, with rationale) before anything changes.
- **Zotero write-back** — value flows back into your library: fetched
  abstracts fill empty Zotero abstract fields, classification suggests 2–4
  tags (existing vocabulary preferred) that you approve per paper, and AI
  summaries can be mirrored as Zotero child notes (updated in place) — so
  the summaries are visible in Zotero even without this app. Everything is
  additive; existing data is never overwritten. Toggles in Settings.
- **Windows installer** (plus Linux/macOS builds) from CI.

## Install

1. Download from [Releases](../../releases): the installer for your platform
   **and** `zotero-notebook.xpi`. (If the page is empty, the latest build is
   still a draft — open it and expand *Assets*.)
2. Install the plugin in **Zotero** (7, 8, or 9): Tools → Plugins → gear icon →
   *Install Plugin From File…* → pick the `.xpi` → restart Zotero.
   You can also export the `.xpi` later from the app: *Settings → Zotero →
   Save plugin file*.
3. Run Zotero Notebook. The onboarding checks that Zotero is running and the
   plugin is detected. Without the plugin the app works in read-only mode.
4. *(Optional, enables AI features)* Add an API key in Settings —
   [Gemini](https://aistudio.google.com/apikey) or
   [Anthropic](https://console.anthropic.com/) — **or** run everything
   locally with no key at all: see
   [Using a local LLM](#using-a-local-llm-no-cloud-no-api-key).
5. *(Optional, enables file moves)* Set **Settings → Files** to your linked
   PDF root folder — typically your ZotMoov destination. Leave it empty to
   move only Zotero collections and never touch files.

## Using a local LLM (no cloud, no API key)

Every AI feature — summaries, full-text summaries, Ask AI chat,
classification, and the filing check — can run entirely on your machine
through any **OpenAI-compatible** server. Your papers never leave your
computer and there is nothing to pay per request.

### Recommended: Ollama

1. **Install [Ollama](https://ollama.com/download)** (Windows installer;
   or `winget install Ollama.Ollama`). After installation Ollama runs as a
   background service on `http://127.0.0.1:11434`.
2. **Download a model** in a terminal:

   ```bash
   ollama pull llama3.1:8b      # good starting point, ~5 GB
   ```

   Larger models give noticeably better classification/chat quality if your
   GPU/RAM allows (e.g. `qwen2.5:14b`, `llama3.1:70b`).
3. In Zotero Notebook open **Settings → AI Provider**, select **Local LLM**,
   and check the fields (defaults match Ollama):
   - *Server URL*: `http://127.0.0.1:11434/v1`
   - *Model*: `llama3.1:8b` (whatever you pulled)
4. **Save changes**, then press **Test** — you should see "Works".

### Alternative: LM Studio (GUI)

1. Install [LM Studio](https://lmstudio.ai/), download a model from its
   built-in browser, and start the **local server** (Developer tab).
2. In Settings → Local LLM set the URL to `http://127.0.0.1:1234/v1` and
   the model to the name LM Studio shows. Any other OpenAI-compatible
   server (llama.cpp `llama-server`, vLLM, …) works the same way — point
   the URL at it.

### Notes on local models

- No API key is needed (a stored key is sent as a Bearer token for servers
  that require one).
- Small local models are weaker than Gemini/Claude at producing the strict
  JSON the classification and filing-check features need. The app embeds
  the JSON schema in the prompt, requests structured output when the server
  supports it, and tolerates markdown-wrapped answers — but with 7–8B
  models expect the occasional skipped paper (it simply stays put / keeps
  its summary). Chat and summaries work well even on small models.
- Local inference is slower than the cloud APIs, especially full-text
  summaries and chat over long papers (the app allows up to 10 minutes per
  request).

### Works with your existing ZotMoov setup

ZotMoov keeps handling newly imported attachments exactly as before. When
you approve a classification, Zotero Notebook performs its own
collection+file move through the companion plugin (inside Zotero,
transactional, rolled back on failure) — files end up in
`<file root>/<Collection>/<Sub-collection>/`, consistent with a
collection-named folder pattern. Keep your ZotMoov folder pattern aligned
with collection names so both tools agree.

## How classification works

1. Open **Unclassified** (papers in the *Unclassified* collection or in no
   collection at all) and press **Classify with AI**.
2. The LLM sees each paper's metadata plus your existing collection paths.
   It must prefer an existing collection and may propose a new one (max 3
   levels) only when nothing fits. Proposals are normalized against the real
   tree — casing is canonicalized and "is this new?" is recomputed, so `llm`
   can never be created next to an existing `LLM`.
3. Nothing moves until you approve. In the review table you can untick rows,
   change targets, or type a brand-new path.
4. Apply: per paper, the plugin creates missing collections, updates
   memberships (removing only the *Unclassified* membership), and moves the
   linked PDF. Failures roll back and the paper stays in *Unclassified*.

## Development

Prerequisites: Rust (stable), Node 22+. The core logic is a headless crate,
fully testable without Zotero or any GUI libraries:

```bash
cargo test -p zn-core      # Zotero/LLM clients vs mock servers + unit tests

cd app
npm install
npm test                   # frontend component tests (vitest)
npx tsc --noEmit           # type check
npm run build:plugin       # package zotero-plugin/ into the bundled .xpi
npm run tauri dev          # run the desktop app (needs WebKitGTK on Linux)
npm run tauri build        # produce installers locally
```

| Path | What it is |
|---|---|
| `core/` | Headless Rust crate: Zotero clients, Gemini/Anthropic clients, classification logic, SQLite sidecar, settings, keychain |
| `app/src/` | React + TypeScript + Tailwind UI |
| `app/src-tauri/` | Thin Tauri 2 shell (commands, watcher, bundling) |
| `zotero-plugin/` | The companion Zotero plugin (bootstrap.js) |
| `docs/ARCHITECTURE.md` | Design + Tauri command table |
| `docs/PLUGIN_API.md` | Plugin wire format (single source of truth) |

## Limitations

- Zotero must be running; the app talks to it on `127.0.0.1:23119`.
- Moves and classification require the companion plugin (read-only mode
  without it).
- Summaries are metadata-based (title/venue/abstract) — PDF text is not
  parsed yet. When a paper has no abstract in Zotero, the app fetches one
  from Crossref → Semantic Scholar → OpenAlex (free, no key); if none is
  found, the summary is generated from the title/venue alone and flagged
  with a "No abstract" badge.
- User library only; group libraries are not supported yet.
