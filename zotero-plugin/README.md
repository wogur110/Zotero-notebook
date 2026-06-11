# Zotero Notebook Connector (Zotero plugin)

Companion plugin for the **Zotero Notebook** desktop app. It adds three
local-only HTTP endpoints to Zotero's built-in server on
`http://127.0.0.1:23119` (never exposed to the network):

| Endpoint | What it does |
|---|---|
| `GET /zotero-notebook/ping` | Liveness/version probe |
| `GET /zotero-notebook/library` | The whole library: collections + items + resolved PDF paths |
| `POST /zotero-notebook/move-item` | Transactional reclassification: create nested collections, update the item's memberships, and move the linked PDF on disk — with rollback on failure |

The desktop app is read-only without this plugin; installing it enables AI
classification and synchronized collection/file moves.

## Install

1. Get `zotero-notebook.xpi` — either from the GitHub Releases page or via
   **Settings → Zotero → Save plugin file (.xpi)…** inside the desktop app.
2. In Zotero 7: **Tools → Plugins** → gear icon → **Install Plugin From
   File…** → select the `.xpi`.
3. Restart Zotero.

Zotero must be running while you use Zotero Notebook.

## Build from source

```bash
cd app
npm install
npm run build:plugin   # writes app/src-tauri/resources/zotero-notebook.xpi
```

The wire format is documented in [`docs/PLUGIN_API.md`](../docs/PLUGIN_API.md);
the desktop app's Rust client and its mock-server tests are written against
that document.
