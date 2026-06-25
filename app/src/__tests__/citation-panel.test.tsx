import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { CitationGraph, Item } from "../types";

vi.mock("../api", () => ({
  fetchCitationGraph: vi.fn(),
  errorMessage: (e: unknown) => String(e),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

import * as api from "../api";
import CitationPanel from "../components/CitationPanel";

const item: Item = {
  key: "I1",
  title: "The Paper",
  itemType: "journalArticle",
  creators: ["A B"],
  year: 2020,
  publication: "Venue",
  doi: "10.1/abc",
  url: null,
  abstractText: null,
  tags: [],
  dateAdded: null,
  collectionKeys: [],
  attachment: null,
};

const graph: CitationGraph = {
  citedByCount: 42,
  fetchFailed: false,
  references: [
    {
      title: "Seminal Missing Work",
      doi: "10.2/sem",
      year: 2015,
      citedByCount: 9000,
      inLibraryKey: null,
    },
    {
      title: "Known Reference",
      doi: "10.3/known",
      year: 2018,
      citedByCount: 10,
      inLibraryKey: "LIBKEY",
    },
  ],
  citations: [
    {
      title: "A Citing Paper",
      doi: null,
      year: 2024,
      citedByCount: 3,
      inLibraryKey: null,
    },
  ],
};

describe("CitationPanel", () => {
  it("renders references, citations, in-library badges, and the seminal callout", async () => {
    (api.fetchCitationGraph as ReturnType<typeof vi.fn>).mockResolvedValue(graph);
    const onOpen = vi.fn();
    render(<CitationPanel item={item} onOpenItem={onOpen} />);

    await screen.findByText("Known Reference");
    expect(
      screen.getByText("Seminal works you're missing"),
    ).toBeInTheDocument();
    expect(screen.getByText(/References \(2\)/)).toBeInTheDocument();
    expect(screen.getByText(/Cited by \(1\)/)).toBeInTheDocument();

    // The in-library entry is clickable and opens that item.
    fireEvent.click(screen.getByText("In library"));
    expect(onOpen).toHaveBeenCalledWith("LIBKEY");
  });

  it("explains when the item has no DOI", async () => {
    (api.fetchCitationGraph as ReturnType<typeof vi.fn>).mockResolvedValue({
      references: [],
      citations: [],
      citedByCount: 0,
      fetchFailed: true,
    });
    render(<CitationPanel item={{ ...item, doi: null }} />);
    await screen.findByText("No citation data");
    expect(screen.getByText(/has no DOI/)).toBeInTheDocument();
  });
});
