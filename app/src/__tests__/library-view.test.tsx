import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import LibraryView from "../views/LibraryView";
import Sidebar from "../components/Sidebar";
import type { Item, Library } from "../types";

const item = (key: string, title: string, collections: string[]): Item => ({
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
  collectionKeys: collections,
  attachment: null,
});

const library: Library = {
  collections: [
    { key: "CV", name: "Computer Vision", parentKey: null },
    { key: "DM", name: "Diffusion Models", parentKey: "CV" },
    { key: "UNC", name: "Unclassified", parentKey: null },
  ],
  items: [
    item("I1", "Paper about vision", ["CV"]),
    item("I2", "Paper about diffusion", ["DM"]),
    item("I3", "Unfiled paper", []),
  ],
  writable: true,
};

describe("LibraryView", () => {
  it("renders all items and opens one on click", () => {
    const onOpen = vi.fn();
    render(
      <LibraryView
        library={library}
        selection={{ kind: "all" }}
        error={null}
        defaultProvider="gemini"
        summarizedKeys={new Set<string>()}
        readingStates={new Map()}
        onOpenItem={onOpen}
        onRetry={() => {}}
        onApplied={() => {}}
        onSummarized={() => {}}
      />,
    );
    expect(screen.getByText("All Papers")).toBeInTheDocument();
    expect(screen.getByText("3 papers")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Paper about vision"));
    expect(onOpen).toHaveBeenCalledWith("I1");
  });

  it("scopes a collection selection to it and its descendants", () => {
    render(
      <LibraryView
        library={library}
        selection={{ kind: "collection", key: "CV" }}
        error={null}
        defaultProvider="gemini"
        summarizedKeys={new Set<string>()}
        readingStates={new Map()}
        onOpenItem={() => {}}
        onRetry={() => {}}
        onApplied={() => {}}
        onSummarized={() => {}}
      />,
    );
    // CV + nested DM = 2 papers; the unfiled one is excluded.
    expect(screen.getByText("2 papers")).toBeInTheDocument();
    expect(screen.queryByText("Unfiled paper")).not.toBeInTheDocument();
  });

  it("shows the error card with a retry action", () => {
    const onRetry = vi.fn();
    render(
      <LibraryView
        library={library}
        selection={{ kind: "all" }}
        error="Zotero is not running"
        defaultProvider="gemini"
        summarizedKeys={new Set<string>()}
        readingStates={new Map()}
        onOpenItem={() => {}}
        onRetry={onRetry}
        onApplied={() => {}}
        onSummarized={() => {}}
      />,
    );
    expect(screen.getByText("Zotero is not running")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
});

describe("Sidebar", () => {
  it("renders the collection tree with counts and fires selections", () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        library={library}
        selection={{ kind: "all" }}
        unclassifiedCount={2}
        queueCount={0}
        starredCount={0}
        onSelect={onSelect}
      />,
    );
    expect(screen.getByText("Computer Vision")).toBeInTheDocument();
    // Roots are expanded by default — the nested collection is visible.
    expect(screen.getByText("Diffusion Models")).toBeInTheDocument();
    // Unclassified badge (warn style when count > 0).
    const badges = screen.getAllByText("2");
    expect(badges.some((el) => el.className.includes("bg-warn-soft"))).toBe(true);

    fireEvent.click(screen.getByText("Diffusion Models"));
    expect(onSelect).toHaveBeenCalledWith({ kind: "collection", key: "DM" });

    fireEvent.click(screen.getByText("Unclassified"));
    expect(onSelect).toHaveBeenCalledWith({ kind: "unclassified" });
  });
});
