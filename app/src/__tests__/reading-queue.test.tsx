import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Item, Library, ReadingState } from "../types";

vi.mock("../api", () => ({
  getSummary: vi.fn(async () => null),
  setReadingState: vi.fn(async () => ({
    itemKey: "I1",
    status: "reading",
    starred: false,
    note: "",
    updatedAt: "2026-06-24T00:00:00Z",
  })),
  errorMessage: (e: unknown) => String(e),
}));

import * as api from "../api";
import LibraryView from "../views/LibraryView";
import ItemDetailModal from "../components/ItemDetailModal";

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
    item("I1", "To-read paper"),
    item("I2", "Reading paper"),
    item("I3", "Finished paper"),
    item("I4", "Untracked paper"),
  ],
  writable: true,
};

const state = (
  itemKey: string,
  status: ReadingState["status"],
  starred = false,
): ReadingState => ({ itemKey, status, starred, note: "", updatedAt: "t" });

const states = new Map<string, ReadingState>([
  ["I1", state("I1", "to_read", true)],
  ["I2", state("I2", "reading")],
  ["I3", state("I3", "read")],
]);

const baseProps = {
  library,
  error: null as string | null,
  defaultProvider: "gemini" as const,
  summarizedKeys: new Set<string>(),
  onRetry: () => {},
  onApplied: () => {},
  onSummarized: () => {},
};

describe("Reading queue view", () => {
  it("lists to-read and reading papers, excluding read and untracked", () => {
    render(
      <LibraryView
        {...baseProps}
        selection={{ kind: "queue" }}
        readingStates={states}
        onOpenItem={() => {}}
      />,
    );
    expect(
      screen.getAllByText("Reading queue").length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("2 papers")).toBeInTheDocument();
    expect(screen.getByText("To-read paper")).toBeInTheDocument();
    expect(screen.getByText("Reading paper")).toBeInTheDocument();
    expect(screen.queryByText("Finished paper")).not.toBeInTheDocument();
    expect(screen.queryByText("Untracked paper")).not.toBeInTheDocument();
  });

  it("shows status badges in a normal view", () => {
    render(
      <LibraryView
        {...baseProps}
        selection={{ kind: "all" }}
        readingStates={states}
        onOpenItem={() => {}}
      />,
    );
    // The status column renders each tracked item's label.
    expect(screen.getByText("To read")).toBeInTheDocument();
    expect(screen.getByText("Reading")).toBeInTheDocument();
    expect(screen.getByText("Read")).toBeInTheDocument();
  });
});

describe("ReadingStatusSection editor", () => {
  it("persists a status pick and reports the change up", async () => {
    const onChanged = vi.fn();
    render(
      <ItemDetailModal
        item={library.items[0]}
        library={library}
        defaultProvider="gemini"
        readingState={null}
        onReadingChanged={onChanged}
        onClose={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Reading" }));
    await waitFor(() =>
      expect(api.setReadingState).toHaveBeenCalledWith("I1", "reading", false, ""),
    );
    expect(onChanged).toHaveBeenCalled();
  });
});
