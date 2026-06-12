# Zotero Notebook Plugin — HTTP API Contract

The companion plugin (`zotero-plugin/`) registers custom endpoints on Zotero's
built-in local HTTP server (default `http://127.0.0.1:23119`) via
`Zotero.Server.Endpoints`. This is the same mechanism Better BibTeX uses; it
requires no extra port and works while Zotero is running.

This document is the **single source of truth** for the wire format. The Rust
client (`core/src/zotero/plugin_api.rs`), the plugin (`zotero-plugin/src/`),
and the mock server used in tests (`core/tests/`) must all match it exactly.

General rules:

- All bodies are JSON, UTF-8. Responses use camelCase keys.
- Errors: non-200 status with body `{"error": "<message>"}`.
- The plugin never talks to the network; it only reads/writes the local
  Zotero database via Zotero's own API, inside transactions.

---

## GET /zotero-notebook/ping

Liveness + version probe.

**200 response**

```json
{ "version": "0.1.0", "zoteroVersion": "7.0.11" }
```

---

## GET /zotero-notebook/library

The whole user library in one shot: every collection and every regular
(top-level, non-attachment, non-note) item, each with its primary PDF
attachment resolved to an absolute path.

**200 response**

```json
{
  "collections": [
    { "key": "ABCD1234", "name": "Computer Vision", "parentKey": null },
    { "key": "EFGH5678", "name": "Diffusion Models", "parentKey": "ABCD1234" }
  ],
  "items": [
    {
      "key": "ITEM0001",
      "title": "Denoising Diffusion Probabilistic Models",
      "itemType": "conferencePaper",
      "creators": ["Jonathan Ho", "Ajay Jain", "Pieter Abbeel"],
      "year": 2020,
      "publication": "NeurIPS",
      "doi": "10.48550/arXiv.2006.11239",
      "url": "https://arxiv.org/abs/2006.11239",
      "abstractText": "We present high quality image synthesis...",
      "tags": ["diffusion", "generative"],
      "dateAdded": "2024-11-02T09:12:33Z",
      "collectionKeys": ["EFGH5678"],
      "attachment": {
        "key": "ATTACH01",
        "title": "Full Text PDF",
        "filename": "Ho et al. - 2020 - DDPM.pdf",
        "contentType": "application/pdf",
        "linkMode": "linked_file",
        "filePath": "C:\\Users\\me\\papers\\Diffusion Models\\Ho et al. - 2020 - DDPM.pdf"
      }
    }
  ]
}
```

Details:

- `creators`: display names in item order (`firstName lastName`, or `name`
  for single-field creators).
- `year`: parsed from the item's `date` field; `null` when unparsable.
- `publication`: `publicationTitle` → `proceedingsTitle` → `conferenceName`
  → `publisher`, first non-empty.
- `attachment`: the first attachment with `contentType ==
  "application/pdf"`; `null` when the item has none.
- `linkMode`: one of `imported_file`, `imported_url`, `linked_file`,
  `linked_url` (Zotero's `Zotero.Attachments.linkModeToName` mapped to
  snake_case).
- `filePath`: result of `attachment.getFilePathAsync()` (absolute, already
  resolves the linked-attachment base directory and the storage folder);
  `null` when the file does not exist on disk.

---

## GET /zotero-notebook/fulltext?itemKey=KEY&maxChars=N

The extracted text of the item's primary PDF, served from Zotero's own
full-text search cache (`.zotero-ft-cache`). When the PDF was never indexed
the plugin asks Zotero to index it on demand before answering.

- `itemKey` (required): the regular item's key.
- `maxChars` (optional): truncate the returned text to this many characters
  (default 80 000, hard cap 200 000). `chars` always reports the full
  length so the caller knows how much was cut.

**200 response**

```json
{ "text": "Denoising diffusion probabilistic models are...", "indexed": true, "truncated": false, "chars": 54321 }
```

When the item has no PDF, or no text could be extracted (scanned PDF
without OCR, missing file):

```json
{ "text": null, "indexed": false, "truncated": false, "chars": 0 }
```

Errors: 400 (missing `itemKey`), 404 (unknown item) with `{"error": ...}`.

---

## POST /zotero-notebook/update-item

Write-back of app-derived data into the Zotero item. Every field is
optional and independent; the endpoint is strictly **additive** — it never
deletes or overwrites user data.

**Request body**

```json
{
  "itemKey": "ITEM0001",
  "abstractIfEmpty": "We present high quality image synthesis...",
  "addTags": ["diffusion models", "image synthesis"],
  "summaryNoteHtml": "<h2>AI Summary — Zotero Notebook</h2><p>...</p>"
}
```

- `abstractIfEmpty`: written to `abstractNote` **only when the field is
  currently empty** — an existing abstract is never touched.
- `addTags`: added to the item's tags; tags that already exist
  (case-insensitive) are skipped, never removed.
- `summaryNoteHtml`: upserted as a child note. The note is identified by
  the marker text `AI Summary — Zotero Notebook` in its content: if a child
  note containing the marker exists its content is replaced in place
  (regenerating a summary never creates a second note); otherwise a new
  child note is created. The caller must include the marker in the HTML.

**200 response**

```json
{ "ok": true, "wroteAbstract": true, "addedTags": ["diffusion models"], "noteKey": "NOTE0001" }
```

`addedTags` lists only the tags actually added; `noteKey` is `null` when no
note was requested. Errors: 400 (bad body), 404 (unknown item) with
`{"error": ...}`.

---

## POST /zotero-notebook/move-item

Atomically reclassify one item: ensure the target collection path exists
(creating nested collections as needed), update the item's collection
memberships, and optionally move the linked PDF on disk.

**Request body**

```json
{
  "itemKey": "ITEM0001",
  "targetPath": ["Computer Vision", "Diffusion Models"],
  "removeFromCollections": ["UNCL0001"],
  "fileRoot": "C:\\Users\\me\\papers"
}
```

- `targetPath`: nested collection names, root → leaf. Names are matched
  **case-insensitively** against existing siblings before creating new
  collections (prevents `LLM` vs `llm` duplicates). Must be non-empty.
- `removeFromCollections`: collection keys to remove the item from (the
  source, typically the "Unclassified" collection key). Other memberships
  are preserved — the move is additive except for these keys.
- `fileRoot` (optional): when present **and** the item's primary PDF is a
  `linked_file`, move the file to
  `<fileRoot>/<targetPath joined by path separator>/<filename>` and update
  the attachment's path. Collection-name path segments are sanitized for the
  filesystem (`/ \ : * ? " < > |` replaced with `_`, trimmed). When absent,
  or when the attachment is stored in Zotero's own storage, no file is
  touched.

**Behavior (sequential saves with explicit rollback — all-or-nothing as
observed by the caller):**

1. Walk `targetPath`, reusing existing collections (case-insensitive name
   match at each level) or creating missing ones (`saveTx` per collection).
2. `item.setCollections([...kept, targetId])`, `item.saveTx()`.
3. Move the file (if requested) and update the attachment path. If any part
   of step 3 fails, the collection change from step 2 is reverted (and a
   half-completed physical move is moved back) before the error is
   returned.

**200 response**

```json
{
  "ok": true,
  "collectionKey": "EFGH5678",
  "newFilePath": "C:\\Users\\me\\papers\\Computer Vision\\Diffusion Models\\Ho et al. - 2020 - DDPM.pdf"
}
```

`newFilePath` is `null` when no file move was requested/possible.

**Error response** — non-200 with `{"error": "..."}`; the plugin must leave
the library unchanged: memberships are reverted, a half-completed physical
move is moved back, and collections created earlier in the same request are
erased again (best-effort — a failure during cleanup is logged via
`Zotero.debug`).

---

## Versioning

Breaking changes to this contract bump the minor version reported by
`/ping`. The app refuses to write when the plugin major.minor is older than
`EXPECTED_PLUGIN_VERSION` (`core/src/zotero/plugin_api.rs`) and surfaces an
"update the plugin" error instead — keep that constant in sync with
`zotero-plugin/manifest.json` when bumping.
