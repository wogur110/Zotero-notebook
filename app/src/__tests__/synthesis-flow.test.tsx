import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Item, Library } from "../types";

vi.mock("../api", () => ({
  chatWithItems: vi.fn(
    async () => "These papers share a focus on diffusion models [Paper 1].",
  ),
  onSynthesisDelta: vi.fn(async () => () => {}),
  errorMessage: (e: unknown) => String(e),
}));

import * as api from "../api";
import SynthesisFlow from "../views/SynthesisFlow";
import LibraryView from "../views/LibraryView";

const item = (key: string, title: string, collections: string[] = []): Item => ({
  key,
  title,
  itemType: "journalArticle",
  creators: ["Ada Lovelace"],
  year: 2021,
  publication: "Venue",
  doi: null,
  url: null,
  abstractText: "An abstract.",
  tags: [],
  dateAdded: "2026-01-01T00:00:00Z",
  collectionKeys: collections,
  attachment: null,
});

const library: Library = {
  collections: [
    { key: "CV", name: "Computer Vision", parentKey: null },
    { key: "UNC", name: "Unclassified", parentKey: null },
  ],
  items: [
    item("I1", "Paper about vision", ["CV"]),
    item("I2", "Paper about diffusion", ["CV"]),
    item("I3", "Paper about NLP", ["CV"]),
  ],
  writable: true,
};

describe("SynthesisFlow", () => {
  it("sends a preset over the scoped item keys and renders the answer", async () => {
    render(
      <SynthesisFlow
        items={[library.items[0], library.items[1]]}
        scopeLabel="All Papers"
        defaultProvider="gemini"
        onClose={() => {}}
      />,
    );
    expect(screen.getByText("Ask across 2 papers")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Overview of these papers"));

    await waitFor(() =>
      expect(
        screen.getByText(/share a focus on diffusion models/),
      ).toBeInTheDocument(),
    );
    expect(api.chatWithItems).toHaveBeenCalledWith(
      ["I1", "I2"],
      [expect.objectContaining({ role: "user" })],
      "gemini",
    );
  });

  it("warns when the scope exceeds the per-request cap", () => {
    const many = Array.from({ length: 60 }, (_, i) =>
      item(`K${i}`, `Paper ${i}`),
    );
    render(
      <SynthesisFlow
        items={many}
        scopeLabel="All Papers"
        defaultProvider="gemini"
        onClose={() => {}}
      />,
    );
    expect(screen.getByText(/using the first 50 of 60/)).toBeInTheDocument();
  });
});

describe("LibraryView · synthesis entry", () => {
  const baseProps = {
    library,
    error: null as string | null,
    defaultProvider: "gemini" as const,
    summarizedKeys: new Set<string>(),
    readingStates: new Map(),
    onOpenItem: () => {},
    onRetry: () => {},
    onApplied: () => {},
    onSummarized: () => {},
  };

  it("synthesizes the whole view when nothing is selected", () => {
    render(<LibraryView {...baseProps} selection={{ kind: "all" }} />);
    fireEvent.click(screen.getByRole("button", { name: /Synthesize/ }));
    // Enters the flow scoped to all 3 papers.
    expect(screen.getByText("Ask across 3 papers")).toBeInTheDocument();
  });

  it("scopes synthesis to the checked subset", () => {
    render(<LibraryView {...baseProps} selection={{ kind: "all" }} />);
    fireEvent.click(screen.getByLabelText("Select Paper about diffusion"));
    // The button reflects the selection and a clear affordance appears.
    expect(
      screen.getByRole("button", { name: /Synthesize 1/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Clear 1 selected")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Synthesize 1/ }));
    expect(screen.getByText("Ask across 1 paper")).toBeInTheDocument();
    expect(screen.getByText("· 1 selected")).toBeInTheDocument();
  });
});
