import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Item, Library } from "../types";

vi.mock("../api", () => ({
  getSummary: vi.fn(async () => null),
  chatWithItem: vi.fn(async () => "The main contribution is the DDPM framework."),
  onChatDelta: vi.fn(async () => () => {}),
  summarizeItem: vi.fn(async () => ({
    itemKey: "I1",
    summary: "A generated summary of the paper.",
    provider: "anthropic",
    model: "claude-opus-4-8",
    createdAt: "2026-06-11T00:00:00Z",
    source: "metadata",
  })),
  classifyItems: vi.fn(async () => [
    {
      itemKey: "I1",
      proposedPath: ["Computer Vision"],
      isNewCollection: false,
      confidence: 0.9,
      rationale: "Clearly a vision paper.",
    },
    {
      itemKey: "I2",
      proposedPath: ["New Topic"],
      isNewCollection: true,
      confidence: 0.4,
      rationale: "Nothing existing fits.",
    },
  ]),
  applyClassifications: vi.fn(async () => [
    { itemKey: "I1", ok: true, error: null, collectionKey: "CV", newFilePath: null },
  ]),
  onClassifyProgress: vi.fn(async () => () => {}),
  onApplyProgress: vi.fn(async () => () => {}),
  revealItemFile: vi.fn(async () => {}),
  openItemPdf: vi.fn(async () => {}),
  openInZotero: vi.fn(async () => {}),
  errorMessage: (e: unknown) => String(e),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

import * as api from "../api";
import ChatPanel from "../components/ChatPanel";
import ItemDetailModal from "../components/ItemDetailModal";
import UnclassifiedView from "../views/UnclassifiedView";

const item = (key: string, title: string): Item => ({
  key,
  title,
  itemType: "conferencePaper",
  creators: ["Jonathan Ho", "Ajay Jain"],
  year: 2020,
  publication: "NeurIPS",
  doi: "10.1/abc",
  url: null,
  abstractText: "An abstract.",
  tags: ["diffusion"],
  dateAdded: "2026-01-01T00:00:00Z",
  collectionKeys: [],
  attachment: {
    key: "A1",
    title: "PDF",
    filename: "paper.pdf",
    contentType: "application/pdf",
    linkMode: "linked_file",
    filePath: "/papers/paper.pdf",
  },
});

const library: Library = {
  collections: [
    { key: "CV", name: "Computer Vision", parentKey: null },
    { key: "UNC", name: "Unclassified", parentKey: null },
  ],
  items: [item("I1", "First paper"), item("I2", "Second paper")],
  writable: true,
};

beforeEach(() => vi.clearAllMocks());

describe("ItemDetailModal", () => {
  it("shows metadata and file actions, and generates a summary", async () => {
    render(
      <ItemDetailModal
        item={library.items[0]}
        library={library}
        defaultProvider="anthropic"
        onClose={() => {}}
      />,
    );
    expect(screen.getByText("First paper")).toBeInTheDocument();
    expect(screen.getByText("NeurIPS")).toBeInTheDocument();
    expect(screen.getByText("paper.pdf")).toBeInTheDocument();
    expect(screen.getByText("Show in Folder")).toBeInTheDocument();

    // No summary stored → generate button appears.
    const btn = await screen.findByRole("button", { name: /Generate summary/ });
    fireEvent.click(btn);
    await waitFor(() =>
      expect(
        screen.getByText("A generated summary of the paper."),
      ).toBeInTheDocument(),
    );
    expect(api.summarizeItem).toHaveBeenCalledWith("I1", "anthropic", false);
    // hadAbstract: false in the mock → metadata-only warning badge.
    expect(
      screen.getByText(/No abstract — title\/venue only/),
    ).toBeInTheDocument();
  });

  it("reveals the file in the file manager", async () => {
    render(
      <ItemDetailModal
        item={library.items[0]}
        library={library}
        defaultProvider="gemini"
        onClose={() => {}}
      />,
    );
    fireEvent.click(screen.getByText("Show in Folder"));
    await waitFor(() => expect(api.revealItemFile).toHaveBeenCalledWith("I1"));
  });
});

describe("ItemDetailModal Ask AI tab", () => {
  it("switches to the chat tab and asks a question", async () => {
    render(
      <ItemDetailModal
        item={library.items[0]}
        library={library}
        defaultProvider="anthropic"
        onClose={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: /Ask AI/ }));
    expect(
      screen.getByText("Ask anything about this paper"),
    ).toBeInTheDocument();

    // Suggestion chips fire a first question.
    fireEvent.click(
      screen.getByRole("button", { name: "What problem does this paper solve?" }),
    );
    await screen.findByText("The main contribution is the DDPM framework.");
    expect(api.chatWithItem).toHaveBeenCalledWith(
      "I1",
      [{ role: "user", content: "What problem does this paper solve?" }],
      "anthropic",
    );
  });
});

describe("ChatPanel", () => {
  it("sends follow-up questions with the full history", async () => {
    render(<ChatPanel item={library.items[0]} defaultProvider="gemini" />);

    const input = screen.getByPlaceholderText("Ask about this paper…");
    fireEvent.change(input, { target: { value: "First question?" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("The main contribution is the DDPM framework.");

    fireEvent.change(
      screen.getByPlaceholderText("Ask about this paper…"),
      { target: { value: "And a follow-up?" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() =>
      expect(api.chatWithItem).toHaveBeenLastCalledWith(
        "I1",
        [
          { role: "user", content: "First question?" },
          {
            role: "assistant",
            content: "The main contribution is the DDPM framework.",
          },
          { role: "user", content: "And a follow-up?" },
        ],
        "gemini",
      ),
    );
  });
});

describe("UnclassifiedView classify flow", () => {
  it("classifies, shows the review screen, and applies only checked rows", async () => {
    const onApplied = vi.fn();
    render(
      <UnclassifiedView
        library={library}
        items={library.items}
        writable
        defaultProvider="gemini"
        onOpenItem={() => {}}
        onApplied={onApplied}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: /Classify 2 papers with AI/ }),
    );

    // Review phase with both proposals.
    await screen.findByText("Review proposals");
    expect(screen.getByText("First paper")).toBeInTheDocument();
    expect(screen.getByText("Second paper")).toBeInTheDocument();
    expect(api.classifyItems).toHaveBeenCalledWith(["I1", "I2"], "gemini");

    // Untick the second proposal — only the first should be applied.
    fireEvent.click(screen.getByLabelText("Include Second paper"));
    fireEvent.click(screen.getByRole("button", { name: /Apply 1 move/ }));

    await screen.findByText(/1 paper classified/);
    expect(api.applyClassifications).toHaveBeenCalledWith([
      { itemKey: "I1", targetPath: ["Computer Vision"] },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Back to library" }));
    expect(onApplied).toHaveBeenCalled();
  });

  it("disables the classify button in read-only mode", () => {
    render(
      <UnclassifiedView
        library={{ ...library, writable: false }}
        items={library.items}
        writable={false}
        defaultProvider="gemini"
        onOpenItem={() => {}}
        onApplied={() => {}}
      />,
    );
    expect(
      screen.getByRole("button", { name: /Classify 2 papers with AI/ }),
    ).toBeDisabled();
  });
});
