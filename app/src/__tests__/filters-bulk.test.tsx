import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EMPTY_FILTER, filterItems } from "../lib/library";
import LibraryView from "../views/LibraryView";
import type { Attachment, Item, Library, ReadingState } from "../types";

const pdf: Attachment = {
  key: "att",
  title: "PDF",
  filename: "a.pdf",
  contentType: "application/pdf",
  linkMode: "linked_file",
  filePath: "/a.pdf",
};

const item = (key: string, over: Partial<Item> = {}): Item => ({
  key,
  title: `Paper ${key}`,
  itemType: "journalArticle",
  creators: [],
  year: 2020,
  publication: null,
  doi: null,
  url: null,
  abstractText: null,
  tags: [],
  dateAdded: null,
  collectionKeys: ["CV"],
  attachment: null,
  ...over,
});

const items: Item[] = [
  item("A", { tags: ["nlp"], year: 2020, attachment: pdf }),
  item("B", { tags: ["cv"], year: 2021 }),
];
const states = new Map<string, ReadingState>([
  ["A", { itemKey: "A", status: "read", starred: true, note: "", updatedAt: "t" }],
]);
const summarizedKeys = new Set(["A"]);
const ctx = { readingStates: states, summarizedKeys };
const keys = (xs: Item[]) => xs.map((i) => i.key);

describe("filterItems", () => {
  it("filters by each facet", () => {
    expect(keys(filterItems(items, { ...EMPTY_FILTER, status: "read" }, ctx))).toEqual(["A"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, status: "none" }, ctx))).toEqual(["B"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, starred: true }, ctx))).toEqual(["A"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, summary: "missing" }, ctx))).toEqual(["B"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, pdf: true }, ctx))).toEqual(["A"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, tag: "cv" }, ctx))).toEqual(["B"]);
    expect(keys(filterItems(items, { ...EMPTY_FILTER, year: 2021 }, ctx))).toEqual(["B"]);
  });
});

const library: Library = {
  collections: [{ key: "CV", name: "Computer Vision", parentKey: null }],
  items,
  writable: true,
};

const base = {
  library,
  error: null as string | null,
  defaultProvider: "gemini" as const,
  summarizedKeys,
  summaries: new Map(),
  readingStates: states,
  onOpenItem: () => {},
  onRetry: () => {},
  onApplied: () => {},
  onSummarized: () => {},
};

describe("LibraryView · filters + bulk actions", () => {
  it("narrows the list via the status filter", () => {
    render(<LibraryView {...base} selection={{ kind: "all" }} />);
    expect(screen.getByText("Paper A")).toBeInTheDocument();
    expect(screen.getByText("Paper B")).toBeInTheDocument();
    // Keep only "Read" papers → A.
    fireEvent.change(screen.getByLabelText("Reading status filter"), {
      target: { value: "read" },
    });
    expect(screen.getByText("Paper A")).toBeInTheDocument();
    expect(screen.queryByText("Paper B")).not.toBeInTheDocument();
    expect(screen.getByText(/1 paper of 2/)).toBeInTheDocument();
  });

  it("runs a bulk status change over the selection", () => {
    const onBulkStatus = vi.fn();
    render(
      <LibraryView
        {...base}
        selection={{ kind: "all" }}
        onBulkStatus={onBulkStatus}
      />,
    );
    fireEvent.click(screen.getByLabelText("Select Paper B"));
    // The bulk bar appears; mark the selection as Read.
    fireEvent.click(screen.getByRole("button", { name: "Read" }));
    expect(onBulkStatus).toHaveBeenCalledWith(["B"], "read");
  });
});
