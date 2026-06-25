import { useEffect, useMemo, useRef, useState } from "react";
import type { Selection } from "../App";
import type { Library } from "../types";
import { buildTree, type CollectionNode } from "../lib/library";
import {
  IconBookmark,
  IconChevronRight,
  IconFolder,
  IconFolderOpen,
  IconInbox,
  IconLibrary,
} from "./icons";

interface Props {
  library: Library;
  selection: Selection;
  unclassifiedCount: number;
  queueCount: number;
  onSelect: (sel: Selection) => void;
}

export default function Sidebar({
  library,
  selection,
  unclassifiedCount,
  queueCount,
  onSelect,
}: Props) {
  // The top-level "Unclassified" collection is represented by the dedicated
  // row above the tree — hide it from the Collections section.
  const tree = useMemo(
    () =>
      buildTree(library).filter(
        (n) =>
          !(
            n.collection.parentKey === null &&
            n.collection.name.trim().toLowerCase() === "unclassified"
          ),
      ),
    [library],
  );
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(tree.map((n) => n.collection.key)),
  );
  const userToggled = useRef(false);

  // The library loads asynchronously after mount; until the user has
  // interacted with the tree, keep root collections expanded by default.
  useEffect(() => {
    if (!userToggled.current) {
      setExpanded(new Set(tree.map((n) => n.collection.key)));
    }
  }, [tree]);

  const toggle = (key: string) => {
    userToggled.current = true;
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  return (
    <aside className="flex h-full w-[264px] shrink-0 flex-col border-r border-edge bg-surface">
      <div className="flex items-center gap-2.5 px-4 pt-4 pb-3">
        <div className="flex h-7 w-7 items-center justify-center rounded-md bg-accent text-accent-text">
          <IconLibrary size={15} />
        </div>
        <span className="text-[15px] font-semibold tracking-tight">
          Zotero Notebook
        </span>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto pb-4">
        <div className="space-y-0.5 px-2">
          <SidebarRow
            icon={<IconLibrary size={16} />}
            label="All Papers"
            count={library.items.length}
            selected={selection.kind === "all"}
            onClick={() => onSelect({ kind: "all" })}
          />
          <SidebarRow
            icon={<IconInbox size={16} />}
            label="Unclassified"
            selected={selection.kind === "unclassified"}
            onClick={() => onSelect({ kind: "unclassified" })}
            trailing={
              <span
                className={`badge ${
                  unclassifiedCount > 0
                    ? "bg-warn-soft text-warn"
                    : "bg-inset text-faint"
                }`}
              >
                {unclassifiedCount}
              </span>
            }
          />
          <SidebarRow
            icon={<IconBookmark size={16} />}
            label="Reading queue"
            selected={selection.kind === "queue"}
            onClick={() => onSelect({ kind: "queue" })}
            trailing={
              queueCount > 0 ? (
                <span className="badge bg-accent-soft text-accent">
                  {queueCount}
                </span>
              ) : undefined
            }
          />
        </div>

        <div className="mt-5 px-4 text-[11px] font-semibold uppercase tracking-wider text-faint">
          Collections
        </div>
        <div className="mt-1 space-y-0.5 px-2">
          {tree.length === 0 ? (
            <p className="px-2 py-3 text-xs text-faint">
              No collections yet — they appear here as soon as Zotero is
              connected.
            </p>
          ) : (
            tree.map((node) => (
              <TreeRow
                key={node.collection.key}
                node={node}
                depth={0}
                selection={selection}
                expanded={expanded}
                onToggle={toggle}
                onSelect={onSelect}
              />
            ))
          )}
        </div>
      </nav>
    </aside>
  );
}

function SidebarRow({
  icon,
  label,
  count,
  selected,
  trailing,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  selected: boolean;
  trailing?: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-sm transition-colors ${
        selected
          ? "bg-accent-soft font-medium text-accent"
          : "text-text hover:bg-inset"
      }`}
    >
      <span className={selected ? "text-accent" : "text-muted"}>{icon}</span>
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {trailing ??
        (count !== undefined && (
          <span className="text-xs text-faint">{count}</span>
        ))}
    </button>
  );
}

function TreeRow({
  node,
  depth,
  selection,
  expanded,
  onToggle,
  onSelect,
}: {
  node: CollectionNode;
  depth: number;
  selection: Selection;
  expanded: Set<string>;
  onToggle: (key: string) => void;
  onSelect: (sel: Selection) => void;
}) {
  const key = node.collection.key;
  const isOpen = expanded.has(key);
  const isSelected =
    selection.kind === "collection" && selection.key === key;
  const hasChildren = node.children.length > 0;

  return (
    <div>
      <div
        className={`group flex h-8 items-center gap-1 rounded-md pr-2 transition-colors ${
          isSelected
            ? "bg-accent-soft font-medium text-accent"
            : "text-text hover:bg-inset"
        }`}
        style={{ paddingLeft: 4 + depth * 14 }}
      >
        <button
          aria-label={isOpen ? "Collapse" : "Expand"}
          onClick={() => hasChildren && onToggle(key)}
          className={`flex h-5 w-5 shrink-0 items-center justify-center rounded text-faint ${
            hasChildren ? "hover:text-text" : "opacity-0"
          }`}
          tabIndex={hasChildren ? 0 : -1}
        >
          <IconChevronRight
            size={13}
            className={`transition-transform duration-150 ${isOpen ? "rotate-90" : ""}`}
          />
        </button>
        <button
          onClick={() => onSelect({ kind: "collection", key })}
          className="flex min-w-0 flex-1 items-center gap-2 py-1 text-left text-sm"
        >
          <span className={isSelected ? "text-accent" : "text-muted"}>
            {isOpen && hasChildren ? (
              <IconFolderOpen size={15} />
            ) : (
              <IconFolder size={15} />
            )}
          </span>
          <span className="min-w-0 flex-1 truncate">{node.collection.name}</span>
          <span className="text-xs text-faint">{node.totalCount}</span>
        </button>
      </div>
      {isOpen &&
        node.children.map((child) => (
          <TreeRow
            key={child.collection.key}
            node={child}
            depth={depth + 1}
            selection={selection}
            expanded={expanded}
            onToggle={onToggle}
            onSelect={onSelect}
          />
        ))}
    </div>
  );
}
