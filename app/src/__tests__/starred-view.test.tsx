import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import LibraryView from "../views/LibraryView";
import type { Item, Library, ReadingState } from "../types";

const item = (key: string, title: string): Item => ({
  key,
  title,
  itemType: "journalArticle",
  creators: ["Ada Lovelace"],
  year: 2021,
  publication: "Venue",
  doi: null,
  url: null,
  abstractText: null,
  tags: [],
  dateAdded: "2026-01-01T00:00:00Z",
  collectionKeys: ["CV"],
  attachment: null,
});

const library: Library = {
  collections: [{ key: "CV", name: "Computer Vision", parentKey: null }],
  items: [
    item("A", "Alpha"),
    item("B", "Beta"),
    item("C", "Gamma"),
    item("D", "Delta"),
  ],
  writable: true,
};

const st = (
  itemKey: string,
  status: ReadingState["status"],
  starred: boolean,
): ReadingState => ({ itemKey, status, starred, note: "", updatedAt: "t" });

// A: starred + to_read · B: starred + no status · C: reading (no star) · D: untracked
const states = new Map<string, ReadingState>([
  ["A", st("A", "to_read", true)],
  ["B", st("B", null, true)],
  ["C", st("C", "reading", false)],
]);

const base = {
  library,
  error: null as string | null,
  defaultProvider: "gemini" as const,
  summarizedKeys: new Set<string>(),
  summaries: new Map(),
  readingStates: states,
  onRetry: () => {},
  onApplied: () => {},
  onSummarized: () => {},
  onOpenItem: () => {},
};

describe("Starred view", () => {
  it("lists every starred paper regardless of reading status", () => {
    render(<LibraryView {...base} selection={{ kind: "starred" }} />);
    expect(screen.getByText("Starred")).toBeInTheDocument();
    expect(screen.getByText("2 papers")).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument(); // starred + to_read
    expect(screen.getByText("Beta")).toBeInTheDocument(); // starred, no status
    expect(screen.queryByText("Gamma")).not.toBeInTheDocument(); // reading, not starred
    expect(screen.queryByText("Delta")).not.toBeInTheDocument(); // untracked
  });

  it("keeps a star-only paper out of the Reading queue (decoupled)", () => {
    render(<LibraryView {...base} selection={{ kind: "queue" }} />);
    // Queue = explicit to_read/reading only.
    expect(screen.getByText("2 papers")).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Gamma")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument(); // starred but no status
  });
});

describe("Quick-star from a list row", () => {
  it("toggles the star via onToggleStar", () => {
    const onToggleStar = vi.fn();
    render(
      <LibraryView
        {...base}
        selection={{ kind: "all" }}
        onToggleStar={onToggleStar}
      />,
    );
    // Delta is untracked → its row offers a "Star Delta" control.
    fireEvent.click(screen.getByLabelText("Star Delta"));
    expect(onToggleStar).toHaveBeenCalledWith("D");
    // Alpha is already starred → its control unstars.
    fireEvent.click(screen.getByLabelText("Unstar Alpha"));
    expect(onToggleStar).toHaveBeenCalledWith("A");
  });
});
