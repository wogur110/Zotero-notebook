// Pure helpers over the Library snapshot. No Tauri imports — unit-testable.

import {
  UNCLASSIFIED_COLLECTION,
  type Collection,
  type Item,
  type Library,
  type ReadingState,
  type ReadingStatus,
} from "../types";

export interface CollectionNode {
  collection: Collection;
  children: CollectionNode[];
  /** Items directly in this collection. */
  itemCount: number;
  /** Items in this collection or any descendant. */
  totalCount: number;
}

const byName = (a: CollectionNode, b: CollectionNode) =>
  a.collection.name.localeCompare(b.collection.name, undefined, {
    sensitivity: "base",
    numeric: true,
  });

/** Build the nested collection tree, sorted by name at every level. */
export function buildTree(library: Library): CollectionNode[] {
  const nodes = new Map<string, CollectionNode>();
  for (const c of library.collections) {
    nodes.set(c.key, { collection: c, children: [], itemCount: 0, totalCount: 0 });
  }
  for (const item of library.items) {
    for (const key of item.collectionKeys) {
      const n = nodes.get(key);
      if (n) n.itemCount += 1;
    }
  }
  const roots: CollectionNode[] = [];
  for (const n of nodes.values()) {
    const parent = n.collection.parentKey ? nodes.get(n.collection.parentKey) : undefined;
    if (parent) parent.children.push(n);
    else roots.push(n);
  }
  const total = (n: CollectionNode): number => {
    n.children.sort(byName);
    n.totalCount = n.itemCount + n.children.reduce((s, c) => s + total(c), 0);
    return n.totalCount;
  };
  roots.forEach(total);
  roots.sort(byName);
  return roots;
}

/** A collection key plus all of its descendants' keys. */
export function descendantKeys(library: Library, rootKey: string): Set<string> {
  const childrenOf = new Map<string, string[]>();
  for (const c of library.collections) {
    if (!c.parentKey) continue;
    const arr = childrenOf.get(c.parentKey) ?? [];
    arr.push(c.key);
    childrenOf.set(c.parentKey, arr);
  }
  const keys = new Set<string>([rootKey]);
  const stack = [rootKey];
  while (stack.length) {
    for (const child of childrenOf.get(stack.pop()!) ?? []) {
      if (!keys.has(child)) {
        keys.add(child);
        stack.push(child);
      }
    }
  }
  return keys;
}

/** Items shown for a sidebar selection (collection + descendants). */
export function itemsForCollection(library: Library, key: string): Item[] {
  const keys = descendantKeys(library, key);
  return library.items.filter((i) => i.collectionKeys.some((k) => keys.has(k)));
}

export function findUnclassifiedCollection(library: Library): Collection | null {
  return (
    library.collections.find(
      (c) =>
        c.parentKey === null &&
        c.name.localeCompare(UNCLASSIFIED_COLLECTION, undefined, {
          sensitivity: "base",
        }) === 0,
    ) ?? null
  );
}

/**
 * Unclassified = items in the top-level "Unclassified" collection, plus
 * items that belong to no collection at all.
 */
export function unclassifiedItems(library: Library): Item[] {
  const uc = findUnclassifiedCollection(library);
  return library.items.filter(
    (i) =>
      i.collectionKeys.length === 0 ||
      (uc !== null && i.collectionKeys.includes(uc.key)),
  );
}

/** True when the collection key sits under the top-level "Unclassified". */
export function isUnclassifiedRooted(library: Library, key: string): boolean {
  const path = collectionPath(library, key);
  return (
    path.length > 0 &&
    path[0].trim().toLowerCase() === UNCLASSIFIED_COLLECTION.toLowerCase()
  );
}

/**
 * Items whose filing can be audited: at least one collection membership
 * outside the Unclassified tree. (Everything else belongs to the
 * Unclassified flow.)
 */
export function auditableItems(library: Library, items: Item[]): Item[] {
  return items.filter((i) =>
    i.collectionKeys.some((k) => !isUnclassifiedRooted(library, k)),
  );
}

export function collectionPath(library: Library, key: string): string[] {
  const path: string[] = [];
  let cursor: string | null = key;
  let guard = 0;
  while (cursor && guard++ < 64) {
    const col: Collection | undefined = library.collections.find((c) => c.key === cursor);
    if (!col) break;
    path.unshift(col.name);
    cursor = col.parentKey;
  }
  return path;
}

/** All existing collection paths, root→leaf, for pickers and the classifier. */
export function allPaths(library: Library): string[][] {
  return library.collections.map((c) => collectionPath(library, c.key));
}

/** Display labels for the reading-status values (UI-facing). */
export const READING_STATUS_LABEL: Record<ReadingStatus, string> = {
  to_read: "To read",
  reading: "Reading",
  read: "Read",
};

/**
 * The active reading queue: items explicitly marked "to read" or "reading"
 * (a bare star with no status does NOT pull a paper in here), ordered
 * starred-first, then reading before to-read, then by title.
 */
export function queueItems(
  library: Library,
  states: Map<string, ReadingState>,
): Item[] {
  const rank = (s: ReadingStatus | null): number => (s === "reading" ? 0 : 1);
  return library.items
    .filter((i) => {
      const st = states.get(i.key);
      return st?.status === "to_read" || st?.status === "reading";
    })
    .sort((a, b) => {
      const sa = states.get(a.key)!;
      const sb = states.get(b.key)!;
      if (sa.starred !== sb.starred) return sa.starred ? -1 : 1;
      const r = rank(sa.status) - rank(sb.status);
      if (r !== 0) return r;
      return a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
    });
}

/** Every starred ("important") paper, regardless of reading status. */
export function starredItems(
  library: Library,
  states: Map<string, ReadingState>,
): Item[] {
  return library.items.filter((i) => states.get(i.key)?.starred === true);
}

export function formatAuthors(creators: string[], max = 3): string {
  if (creators.length === 0) return "—";
  if (creators.length <= max) return creators.join(", ");
  return `${creators.slice(0, max).join(", ")} +${creators.length - max}`;
}

export const pathLabel = (path: string[]): string => path.join(" / ");
