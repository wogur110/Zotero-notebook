/* global Zotero, IOUtils, PathUtils */
/*
 * Zotero Notebook Connector — companion plugin for the Zotero Notebook
 * desktop app.
 *
 * Registers three endpoints on Zotero's built-in local HTTP server
 * (127.0.0.1:23119). The wire format is specified in docs/PLUGIN_API.md of
 * the Zotero-notebook repository; the desktop app's Rust client and its
 * tests are written against that document. Keep them in sync.
 *
 * Everything here runs in Zotero's privileged context: never throw out of
 * an endpoint — always return a JSON error body instead.
 */

"use strict";

var ZoteroNotebook = {
  version: "1.1.0",
  registeredPaths: [],
};

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

function jsonResponse(status, payload) {
  return [status, "application/json", JSON.stringify(payload)];
}

function jsonError(status, message) {
  return jsonResponse(status, { error: String(message) });
}

function debug(msg) {
  Zotero.debug("[zotero-notebook] " + msg);
}

/** Make a collection name safe to use as a directory name. */
function sanitizeSegment(name) {
  let s = String(name)
    // eslint-disable-next-line no-control-regex
    .replace(/[/\\:*?"<>|\x00-\x1f]/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^[. ]+|[. ]+$/g, "");
  if (s.length > 80) s = s.slice(0, 80).trim();
  return s.length ? s : "_";
}

function linkModeName(attachment) {
  const A = Zotero.Attachments;
  switch (attachment.attachmentLinkMode) {
    case A.LINK_MODE_IMPORTED_FILE:
      return "imported_file";
    case A.LINK_MODE_IMPORTED_URL:
      return "imported_url";
    case A.LINK_MODE_LINKED_FILE:
      return "linked_file";
    case A.LINK_MODE_LINKED_URL:
      return "linked_url";
    default:
      return "other";
  }
}

function creatorDisplayName(creator) {
  if (creator.fieldMode === 1) {
    return (creator.lastName || "").trim();
  }
  return ((creator.firstName || "") + " " + (creator.lastName || "")).trim();
}

function getFieldOrNull(item, field) {
  try {
    const v = item.getField(field);
    return v && String(v).trim() ? String(v).trim() : null;
  } catch (e) {
    return null;
  }
}

function sqlDateToIso(sqlDate) {
  // Zotero stores "YYYY-MM-DD HH:MM:SS" in UTC.
  if (!sqlDate) return null;
  return sqlDate.replace(" ", "T") + "Z";
}

/**
 * The item's primary PDF: the FIRST attachment with contentType
 * application/pdf, regardless of link mode. This selection rule is part of
 * the wire contract — /library serves it and /move-item must operate on the
 * same attachment.
 */
function primaryPdfItem(item) {
  for (const attachmentID of item.getAttachments()) {
    let att;
    try {
      att = Zotero.Items.get(attachmentID);
    } catch (e) {
      continue;
    }
    if (att && att.attachmentContentType === "application/pdf") return att;
  }
  return null;
}

async function primaryPdfPayload(item) {
  const att = primaryPdfItem(item);
  if (!att) return null;
  let filePath = null;
  try {
    const p = await att.getFilePathAsync();
    filePath = p ? String(p) : null;
  } catch (e) {
    filePath = null;
  }
  return {
    key: att.key,
    title: getFieldOrNull(att, "title") || "PDF",
    filename: att.attachmentFilename || null,
    contentType: att.attachmentContentType || null,
    linkMode: linkModeName(att),
    filePath,
  };
}

async function itemPayload(item) {
  const creators = item
    .getCreators()
    .map(creatorDisplayName)
    .filter((n) => n.length > 0);

  let year = null;
  const dateField = getFieldOrNull(item, "date");
  if (dateField) {
    const parsed = Zotero.Date.strToDate(dateField);
    if (parsed && parsed.year && !isNaN(parseInt(parsed.year, 10))) {
      year = parseInt(parsed.year, 10);
    }
  }

  const publication =
    getFieldOrNull(item, "publicationTitle") ||
    getFieldOrNull(item, "proceedingsTitle") ||
    getFieldOrNull(item, "conferenceName") ||
    getFieldOrNull(item, "publisher");

  const collectionKeys = [];
  for (const collectionID of item.getCollections()) {
    const col = Zotero.Collections.get(collectionID);
    if (col) collectionKeys.push(col.key);
  }

  return {
    key: item.key,
    title: getFieldOrNull(item, "title") || "(untitled)",
    itemType: Zotero.ItemTypes.getName(item.itemTypeID),
    creators,
    year,
    publication,
    doi: getFieldOrNull(item, "DOI"),
    url: getFieldOrNull(item, "url"),
    abstractText: getFieldOrNull(item, "abstractNote"),
    tags: item.getTags().map((t) => t.tag),
    dateAdded: sqlDateToIso(item.dateAdded),
    collectionKeys,
    attachment: await primaryPdfPayload(item),
  };
}

async function buildLibraryPayload() {
  const libraryID = Zotero.Libraries.userLibraryID;

  const collections = Zotero.Collections.getByLibrary(libraryID, true).map(
    (col) => ({
      key: col.key,
      name: col.name,
      parentKey: col.parentKey ? col.parentKey : null,
    })
  );

  const allItems = await Zotero.Items.getAll(libraryID, true, false);
  const items = [];
  for (const item of allItems) {
    try {
      if (!item.isRegularItem() || item.deleted) continue;
      items.push(await itemPayload(item));
    } catch (e) {
      debug("skipping item " + (item && item.key) + ": " + e);
    }
  }

  return { collections, items };
}

// ---------------------------------------------------------------------
// Full text
// ---------------------------------------------------------------------

const FULLTEXT_DEFAULT_MAX = 80000;
const FULLTEXT_HARD_MAX = 200000;

/**
 * Read the text Zotero already extracted for search (`.zotero-ft-cache`).
 * Location mirrors Zotero.Fulltext's write logic: the item's storage
 * directory for linked files, the file's own folder otherwise.
 * Returns null when no cache exists.
 */
async function readFulltextCache(attachment) {
  try {
    let parentDir;
    if (
      attachment.attachmentLinkMode === Zotero.Attachments.LINK_MODE_LINKED_FILE
    ) {
      parentDir = Zotero.Attachments.getStorageDirectory(attachment).path;
    } else {
      const filePath = await attachment.getFilePathAsync();
      if (!filePath) return null;
      parentDir = PathUtils.parent(filePath);
    }
    const cachePath = PathUtils.join(
      parentDir,
      Zotero.Fulltext.fulltextCacheFile
    );
    if (!(await IOUtils.exists(cachePath))) return null;
    const text = await Zotero.File.getContentsAsync(cachePath);
    return typeof text === "string" ? text : String(text);
  } catch (e) {
    debug("fulltext cache read failed: " + e);
    return null;
  }
}

async function getFulltext(searchParams) {
  const itemKey = (searchParams.get("itemKey") || "").trim();
  if (!itemKey) {
    return jsonError(400, "itemKey query parameter is required");
  }
  let maxChars = parseInt(searchParams.get("maxChars") || "", 10);
  if (!Number.isFinite(maxChars) || maxChars <= 0) {
    maxChars = FULLTEXT_DEFAULT_MAX;
  }
  maxChars = Math.min(maxChars, FULLTEXT_HARD_MAX);

  const libraryID = Zotero.Libraries.userLibraryID;
  const item = Zotero.Items.getByLibraryAndKey(libraryID, itemKey);
  if (!item || !item.isRegularItem()) {
    return jsonError(404, "no item with key '" + itemKey + "' in the user library");
  }
  const attachment = primaryPdfItem(item);
  const empty = { text: null, indexed: false, truncated: false, chars: 0 };
  if (!attachment) {
    return jsonResponse(200, empty);
  }

  let text = await readFulltextCache(attachment);
  if (text === null) {
    // Not indexed yet (e.g. freshly added PDF) — ask Zotero to extract it.
    try {
      await Zotero.Fulltext.indexItems([attachment.id], {});
    } catch (e) {
      debug("on-demand indexing failed for " + itemKey + ": " + e);
    }
    text = await readFulltextCache(attachment);
  }
  if (text === null || !text.trim()) {
    return jsonResponse(200, empty);
  }

  const chars = text.length;
  const truncated = chars > maxChars;
  return jsonResponse(200, {
    text: truncated ? text.slice(0, maxChars) : text,
    indexed: true,
    truncated,
    chars,
  });
}

/**
 * Find or create the nested collection described by `targetPath`.
 * Existing names are matched case-insensitively so the LLM can never create
 * an "llm" next to an existing "LLM". Newly created collections are pushed
 * onto `createdOut` (in creation order) so failures later in the move can
 * erase them again.
 */
async function ensureCollectionPath(libraryID, targetPath, createdOut) {
  let parent = null; // Zotero.Collection or null at root level
  for (const rawSegment of targetPath) {
    const segment = String(rawSegment).trim();
    const siblings = parent
      ? Zotero.Collections.getByParent(parent.id)
      : Zotero.Collections.getByLibrary(libraryID, false);
    let next = siblings.find(
      (c) => c.name.trim().toLowerCase() === segment.toLowerCase()
    );
    if (!next) {
      next = new Zotero.Collection();
      next.libraryID = libraryID;
      next.name = segment;
      if (parent) next.parentKey = parent.key;
      await next.saveTx();
      createdOut.push(next);
      debug("created collection '" + segment + "' (" + next.key + ")");
    }
    parent = next;
  }
  return parent;
}

/** Best-effort removal of collections created during a failed move. */
async function eraseCreatedCollections(created) {
  for (const col of created.slice().reverse()) {
    try {
      await col.eraseTx();
      debug("rolled back created collection '" + col.name + "'");
    } catch (e) {
      debug("FAILED to erase created collection '" + col.name + "': " + e);
    }
  }
}

/**
 * The /move-item operation. Sequential saves with explicit rollback so the
 * caller observes all-or-nothing behavior:
 *   1. ensure the target collection path exists,
 *   2. update the item's collection memberships,
 *   3. optionally move the linked PDF on disk and update its path.
 * Any failure reverts the earlier steps (memberships, half-done file moves,
 * and collections created in step 1) before the error response is sent.
 */
async function moveItem(body) {
  if (!body || typeof body !== "object") {
    return jsonError(400, "request body must be a JSON object");
  }
  const { itemKey, targetPath, removeFromCollections, fileRoot } = body;
  if (typeof itemKey !== "string" || !itemKey.trim()) {
    return jsonError(400, "itemKey must be a non-empty string");
  }
  if (
    !Array.isArray(targetPath) ||
    targetPath.length === 0 ||
    targetPath.some((s) => typeof s !== "string" || !s.trim())
  ) {
    return jsonError(400, "targetPath must be a non-empty array of non-empty strings");
  }
  const removeKeys = Array.isArray(removeFromCollections)
    ? removeFromCollections.filter((k) => typeof k === "string")
    : [];

  const libraryID = Zotero.Libraries.userLibraryID;
  const item = Zotero.Items.getByLibraryAndKey(libraryID, itemKey.trim());
  if (!item || !item.isRegularItem()) {
    return jsonError(404, "no item with key '" + itemKey + "' in the user library");
  }

  // --- step 1: target collection -----------------------------------
  const created = [];
  let target;
  try {
    target = await ensureCollectionPath(libraryID, targetPath, created);
  } catch (e) {
    await eraseCreatedCollections(created);
    return jsonError(500, "creating the target collection failed: " + (e.message || e));
  }

  // --- step 2: memberships ------------------------------------------
  // includeTrashed=true: memberships in trashed collections must survive
  // the round trip untouched (they are invisible to the app anyway).
  const previousCollectionIDs = item.getCollections(true);
  const removeIDs = new Set();
  for (const key of removeKeys) {
    const col = Zotero.Collections.getByLibraryAndKey(libraryID, key);
    if (col) removeIDs.add(col.id);
  }
  const newCollectionIDs = previousCollectionIDs
    .filter((id) => !removeIDs.has(id))
    .concat([target.id])
    // dedupe
    .filter((id, i, arr) => arr.indexOf(id) === i);

  try {
    item.setCollections(newCollectionIDs);
    await item.saveTx();
  } catch (e) {
    await eraseCreatedCollections(created);
    return jsonError(500, "updating collection memberships failed: " + (e.message || e));
  }

  const revertMemberships = async () => {
    try {
      item.setCollections(previousCollectionIDs);
      await item.saveTx();
    } catch (e) {
      debug("FAILED to revert memberships for " + itemKey + ": " + e);
    }
  };

  // --- step 3: optional physical file move --------------------------
  // Operates on the SAME attachment /library serves as the primary PDF;
  // files are only touched when that attachment is a linked file.
  let newFilePath = null;
  if (typeof fileRoot === "string" && fileRoot.trim()) {
    const attachment = primaryPdfItem(item);
    if (
      attachment &&
      attachment.attachmentLinkMode === Zotero.Attachments.LINK_MODE_LINKED_FILE
    ) {
      let moved = false;
      let currentPath = null;
      let destPath = null;
      const previousAttachmentPath = attachment.attachmentPath;
      try {
        const p = await attachment.getFilePathAsync();
        currentPath = p ? String(p) : null;

        if (currentPath) {
          const segments = targetPath.map(sanitizeSegment);
          const destDir = PathUtils.join(fileRoot.trim(), ...segments);
          destPath = PathUtils.join(destDir, PathUtils.filename(currentPath));

          if (destPath !== currentPath) {
            if (await IOUtils.exists(destPath)) {
              throw new Error("a different file already exists at " + destPath);
            }
            await Zotero.File.createDirectoryIfMissingAsync(destDir);
            await IOUtils.move(currentPath, destPath);
            moved = true;

            // Store relative to the linked-attachment base directory when
            // one is configured, absolute otherwise.
            let storedPath = destPath;
            try {
              if (
                typeof Zotero.Attachments.getBaseDirectoryRelativePath ===
                "function"
              ) {
                storedPath =
                  Zotero.Attachments.getBaseDirectoryRelativePath(destPath) ||
                  destPath;
              }
            } catch (e) {
              storedPath = destPath;
            }
            attachment.attachmentPath = storedPath;
            await attachment.saveTx();
            newFilePath = destPath;
          } else {
            newFilePath = currentPath;
          }
        }
      } catch (e) {
        // Roll everything back: file first, then memberships, then any
        // collections this request created.
        if (moved) {
          try {
            await IOUtils.move(destPath, currentPath);
          } catch (moveBackErr) {
            debug("FAILED to move file back after error: " + moveBackErr);
          }
          try {
            attachment.attachmentPath = previousAttachmentPath;
            await attachment.saveTx();
          } catch (restoreErr) {
            debug("FAILED to restore attachment path: " + restoreErr);
          }
        }
        await revertMemberships();
        await eraseCreatedCollections(created);
        return jsonError(500, "file move failed: " + (e.message || e));
      }
    }
  }

  return jsonResponse(200, {
    ok: true,
    collectionKey: target.key,
    newFilePath,
  });
}

// ---------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------
// NOTE: Zotero's server dispatches on the DECLARED ARITY of init:
// `init.length === 1` selects the modern requestData/return-tuple style;
// any other arity falls into legacy callback branches where the returned
// tuple is silently discarded and the HTTP request hangs forever. Every
// init below must declare exactly one parameter, even when unused.

function makePingEndpoint() {
  const Endpoint = function () {};
  Endpoint.prototype = {
    supportedMethods: ["GET"],
    permitBookmarklet: false,
    init: function (_requestData) {
      return jsonResponse(200, {
        version: ZoteroNotebook.version,
        zoteroVersion: Zotero.version,
      });
    },
  };
  return Endpoint;
}

function makeLibraryEndpoint() {
  const Endpoint = function () {};
  Endpoint.prototype = {
    supportedMethods: ["GET"],
    permitBookmarklet: false,
    init: async function (_requestData) {
      try {
        const payload = await buildLibraryPayload();
        return jsonResponse(200, payload);
      } catch (e) {
        debug("library endpoint failed: " + e + "\n" + (e && e.stack));
        return jsonError(500, "failed to read the library: " + (e.message || e));
      }
    },
  };
  return Endpoint;
}

function makeFulltextEndpoint() {
  const Endpoint = function () {};
  Endpoint.prototype = {
    supportedMethods: ["GET"],
    permitBookmarklet: false,
    init: async function (requestData) {
      try {
        return await getFulltext(requestData.searchParams);
      } catch (e) {
        debug("fulltext endpoint failed: " + e + "\n" + (e && e.stack));
        return jsonError(500, "failed to read full text: " + (e.message || e));
      }
    },
  };
  return Endpoint;
}

function makeMoveItemEndpoint() {
  const Endpoint = function () {};
  Endpoint.prototype = {
    supportedMethods: ["POST"],
    supportedDataTypes: ["application/json"],
    permitBookmarklet: false,
    init: async function (requestData) {
      try {
        return await moveItem(requestData.data);
      } catch (e) {
        debug("move-item endpoint failed: " + e + "\n" + (e && e.stack));
        return jsonError(500, "move failed: " + (e.message || e));
      }
    },
  };
  return Endpoint;
}

// ---------------------------------------------------------------------
// Bootstrap hooks
// ---------------------------------------------------------------------

function install() {}

function startup({ version }) {
  // Keep the reported version in sync with the manifest.
  if (version) ZoteroNotebook.version = version;

  const endpoints = {
    "/zotero-notebook/ping": makePingEndpoint(),
    "/zotero-notebook/library": makeLibraryEndpoint(),
    "/zotero-notebook/move-item": makeMoveItemEndpoint(),
    "/zotero-notebook/fulltext": makeFulltextEndpoint(),
  };
  for (const [path, endpoint] of Object.entries(endpoints)) {
    Zotero.Server.Endpoints[path] = endpoint;
    ZoteroNotebook.registeredPaths.push(path);
  }
  debug("endpoints registered (v" + ZoteroNotebook.version + ")");
}

function shutdown() {
  for (const path of ZoteroNotebook.registeredPaths) {
    delete Zotero.Server.Endpoints[path];
  }
  ZoteroNotebook.registeredPaths = [];
  debug("endpoints removed");
}

function uninstall() {}
