import { describe, expect, it } from "vitest";
import { buildReviewMarkdown, exportFileName } from "../lib/export";
import type { Item, StoredSummary } from "../types";

const item = (
  key: string,
  title: string,
  extra: Partial<Item> = {},
): Item => ({
  key,
  title,
  itemType: "journalArticle",
  creators: ["Ada Lovelace", "Alan Turing"],
  year: 2021,
  publication: "Venue",
  doi: null,
  url: null,
  abstractText: null,
  tags: [],
  dateAdded: null,
  collectionKeys: [],
  attachment: null,
  ...extra,
});

const summary = (itemKey: string, text: string): StoredSummary => ({
  itemKey,
  summary: text,
  provider: "gemini",
  model: "gemini-2.5-pro",
  createdAt: "2026-06-25T00:00:00Z",
  source: "abstract",
});

describe("buildReviewMarkdown", () => {
  it("includes the narrative and an annotated bibliography", () => {
    const items = [
      item("A", "First Paper", { doi: "10.1/abc", tags: ["nlp"] }),
      item("B", "Second Paper"),
    ];
    const summaries = new Map([["A", summary("A", "A is about X.")]]);
    const md = buildReviewMarkdown({
      title: "My Collection",
      generatedAt: "2026-06-25 10:00",
      items,
      summaries,
      narrative: [
        { role: "user", content: "Compare the methods." },
        { role: "assistant", content: "They both do Y." },
      ],
    });

    expect(md).toContain("# Review — My Collection");
    expect(md).toContain("2 papers");
    // Narrative.
    expect(md).toContain("## Synthesis");
    expect(md).toContain("### Compare the methods.");
    expect(md).toContain("They both do Y.");
    // Bibliography.
    expect(md).toContain("## Annotated bibliography");
    expect(md).toContain("### 1. First Paper");
    expect(md).toContain("Ada Lovelace, Alan Turing");
    expect(md).toContain("DOI: [10.1/abc]");
    expect(md).toContain("A is about X.");
    expect(md).toContain("`nlp`");
    // Missing summary is flagged, not omitted.
    expect(md).toContain("### 2. Second Paper");
    expect(md).toContain("_(no AI summary yet)_");
  });

  it("omits the synthesis section when there is no narrative", () => {
    const md = buildReviewMarkdown({
      title: "X",
      generatedAt: "t",
      items: [item("A", "T")],
      summaries: new Map(),
    });
    expect(md).not.toContain("## Synthesis");
    expect(md).toContain("## Annotated bibliography");
  });

  it("builds a filesystem-safe file name", () => {
    expect(exportFileName("Computer Vision / Diffusion!")).toBe(
      "review-computer-vision-diffusion.md",
    );
    expect(exportFileName("")).toBe("review-papers.md");
  });
});
