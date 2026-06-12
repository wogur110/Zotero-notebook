import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Item, Library } from "../types";

vi.mock("../api", () => ({
  auditItems: vi.fn(async () => [
    {
      itemKey: "I1",
      currentPaths: [["Hardware"]],
      currentKeys: ["HW"],
      proposedPath: ["NLP"],
      isNewCollection: false,
      confidence: 0.85,
      rationale: "This is a transformer paper, not a hardware paper.",
    },
  ]),
  applyClassifications: vi.fn(async () => [
    { itemKey: "I1", ok: true, error: null, collectionKey: "NLP", newFilePath: null },
  ]),
  onAuditProgress: vi.fn(async () => () => {}),
  onApplyProgress: vi.fn(async () => () => {}),
  errorMessage: (e: unknown) => String(e),
}));

import * as api from "../api";
import AuditFlow from "../views/AuditFlow";

const item = (key: string, title: string, collections: string[]): Item => ({
  key,
  title,
  itemType: "conferencePaper",
  creators: ["Ashish Vaswani"],
  year: 2017,
  publication: "NeurIPS",
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
    { key: "HW", name: "Hardware", parentKey: null },
    { key: "NLP", name: "NLP", parentKey: null },
  ],
  items: [
    item("I1", "Attention Is All You Need", ["HW"]),
    item("I2", "A real hardware paper", ["HW"]),
  ],
  writable: true,
};

beforeEach(() => vi.clearAllMocks());

describe("AuditFlow", () => {
  it("confirms, scans, reviews a flagged paper, and applies with replaced memberships", async () => {
    const onApplied = vi.fn();
    const onClose = vi.fn();
    render(
      <AuditFlow
        library={library}
        items={library.items}
        scopeLabel="All Papers"
        defaultProvider="anthropic"
        onOpenItem={() => {}}
        onClose={onClose}
        onApplied={onApplied}
      />,
    );

    // Confirm step shows scope + count, nothing has run yet.
    expect(screen.getByText(/Check filing of 2 papers/)).toBeInTheDocument();
    expect(screen.getByText("All Papers")).toBeInTheDocument();
    expect(api.auditItems).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /Start checking/ }));
    expect(await screen.findByText("Filing check results")).toBeInTheDocument();
    expect(api.auditItems).toHaveBeenCalledWith(["I1", "I2"], "anthropic");

    // 1 of 2 flagged: summary line + current path (struck through) + target.
    expect(screen.getByText(/1 of 2 look correctly filed/)).toBeInTheDocument();
    expect(screen.getByText("Attention Is All You Need")).toBeInTheDocument();
    expect(screen.getByText("Hardware")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Move 1 paper/ }));
    await screen.findByText(/1 paper refiled/);
    expect(api.applyClassifications).toHaveBeenCalledWith([
      { itemKey: "I1", targetPath: ["NLP"], removeCollectionKeys: ["HW"] },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Back to library" }));
    expect(onApplied).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("shows the all-clear state when nothing is flagged", async () => {
    vi.mocked(api.auditItems).mockResolvedValueOnce([]);
    render(
      <AuditFlow
        library={library}
        items={library.items}
        scopeLabel="Hardware"
        defaultProvider="gemini"
        onOpenItem={() => {}}
        onClose={() => {}}
        onApplied={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Start checking/ }));
    expect(
      await screen.findByText("Everything looks well filed"),
    ).toBeInTheDocument();
  });
});
