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
  version: "0.1.0",
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

async function primaryPdfAttachment(item) {
  for (const attachmentID of item.getAttachments()) {
    let att;
    try {
      att = Zotero.Items.get(attachmentID);
    } catch (e) {
      continue;
    }
    if (!att || att.attachmentContentType !== "application/pdf") continue;
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
  return null;
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
    attachment: await primaryPdfAttachment(item),
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

/**
 * Find or create the nested collection described by `targetPath`.
 * Existing names are matched case-insensitively so the LLM can never create
 * an "llm" next to an existing "LLM". Returns the leaf collection.
 */
async function ensureCollectionPath(libraryID, targetPath) {
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
      debug("created collection '" + segment + "' (" + next.key + ")");
    }
    parent = next;
  }
  return parent;
}

/**
 * The /move-item operation. Sequential saves with explicit rollback so the
 * caller observes all-or-nothing behavior:
 *   1. ensure the target collection path exists,
 *   2. update the item's collection memberships,
 *   3. optionally move the linked PDF on disk and update its path.
 * If 3 fails, 2 is reverted (and a half-done physical move is moved back).
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
  const target = await ensureCollectionPath(libraryID, targetPath);

  // --- step 2: memberships ------------------------------------------
  const previousCollectionIDs = item.getCollections();
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

  item.setCollections(newCollectionIDs);
  await item.saveTx();

  const revertMemberships = async () => {
    try {
      item.setCollections(previousCollectionIDs);
      await item.saveTx();
    } catch (e) {
      debug("FAILED to revert memberships for " + itemKey + ": " + e);
    }
  };

  // --- step 3: optional physical file move --------------------------
  let newFilePath = null;
  if (typeof fileRoot === "string" && fileRoot.trim()) {
    const attachment = await (async () => {
      for (const id of item.getAttachments()) {
        const att = Zotero.Items.get(id);
        if (
          att &&
          att.attachmentContentType === "application/pdf" &&
          att.attachmentLinkMode === Zotero.Attachments.LINK_MODE_LINKED_FILE
        ) {
          return att;
        }
      }
      return null;
    })();

    if (attachment) {
      let currentPath = null;
      try {
        const p = await attachment.getFilePathAsync();
        currentPath = p ? String(p) : null;
      } catch (e) {
        currentPath = null;
      }

      if (currentPath) {
        const segments = targetPath.map(sanitizeSegment);
        const destDir = PathUtils.join(fileRoot.trim(), ...segments);
        const destPath = PathUtils.join(destDir, PathUtils.filename(currentPath));

        if (destPath !== currentPath) {
          let moved = false;
          const previousAttachmentPath = attachment.attachmentPath;
          try {
            if (await IOUtils.exists(destPath)) {
              throw new Error(
                "a different file already exists at " + destPath
              );
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
          } catch (e) {
            // Roll everything back: file first, then memberships.
            if (moved) {
              try {
                await IOUtils.move(destPath, currentPath);
              } catch (moveBackErr) {
                debug(
                  "FAILED to move file back after error: " + moveBackErr
                );
              }
              try {
                attachment.attachmentPath = previousAttachmentPath;
                await attachment.saveTx();
              } catch (restoreErr) {
                debug("FAILED to restore attachment path: " + restoreErr);
              }
            }
            await revertMemberships();
            return jsonError(500, "file move failed: " + (e.message || e));
          }
        } else {
          newFilePath = currentPath;
        }
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

function makePingEndpoint() {
  const Endpoint = function () {};
  Endpoint.prototype = {
    supportedMethods: ["GET"],
    permitBookmarklet: false,
    init: function () {
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
    init: async function () {
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
