// A dismissible banner shown under the topbar when new unclassified papers
// appear (detected by diffing the library across refreshes). It only routes
// into the existing Unclassified review flow — it never moves anything.

import { IconInbox, IconX } from "./icons";

interface Props {
  count: number;
  onClassify: () => void;
  onDismiss: () => void;
}

export default function NewImportsBanner({
  count,
  onClassify,
  onDismiss,
}: Props) {
  return (
    <div className="flex items-center gap-3 border-b border-edge bg-accent-soft px-6 py-2 text-sm text-accent">
      <IconInbox size={16} />
      <span className="flex-1 text-text">
        <span className="font-medium">
          {count} new {count === 1 ? "paper" : "papers"}
        </span>{" "}
        imported into Zotero {count === 1 ? "is" : "are"} still unclassified.
      </span>
      <button className="btn-primary py-1! text-xs" onClick={onClassify}>
        Classify now
      </button>
      <button
        className="btn-ghost h-7 w-7 px-0!"
        aria-label="Dismiss"
        onClick={onDismiss}
      >
        <IconX size={14} />
      </button>
    </div>
  );
}
