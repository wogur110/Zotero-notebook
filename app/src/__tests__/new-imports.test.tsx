import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Item, Library } from "../types";

const item = (key: string): Item => ({
  key,
  title: `Paper ${key}`,
  itemType: "journalArticle",
  creators: [],
  year: null,
  publication: null,
  doi: null,
  url: null,
  abstractText: null,
  tags: [],
  dateAdded: null,
  collectionKeys: [], // unfiled => unclassified
  attachment: null,
});

const lib = (keys: string[]): Library => ({
  collections: [{ key: "UNC", name: "Unclassified", parentKey: null }],
  items: keys.map(item),
  writable: true,
});

vi.mock("../api", () => ({
  getStatus: vi.fn(async () => ({
    running: true,
    pluginInstalled: true,
    pluginVersion: "1.3.0",
    hint: null,
  })),
  getLibrary: vi.fn(),
  getSettings: vi.fn(async () => ({
    defaultProvider: "gemini",
    geminiModel: "gemini-2.5-pro",
    anthropicModel: "claude-opus-4-8",
    localBaseUrl: "",
    localModel: "",
    zoteroBaseUrl: "",
    writeBackAbstracts: true,
    syncSummaryNotes: true,
    fileRoot: null,
  })),
  getAllSummaries: vi.fn(async () => []),
  getReadingStates: vi.fn(async () => []),
  getUsageSummary: vi.fn(async () => ({
    totalInputTokens: 0,
    totalOutputTokens: 0,
    totalCostUsd: 0,
    operationCount: 0,
  })),
  onZoteroStatus: vi.fn(async () => () => {}),
  onUsageUpdate: vi.fn(async () => () => {}),
  errorMessage: (e: unknown) => String(e),
}));

import * as api from "../api";
import App from "../App";

const getLibrary = api.getLibrary as ReturnType<typeof vi.fn>;

beforeEach(() => {
  localStorage.setItem("zn-onboarded", "1");
  getLibrary.mockReset();
});

describe("New-import detection", () => {
  it("flags newly-appeared unclassified papers on refresh, and dismisses", async () => {
    // Baseline load: one unclassified paper, no banner.
    getLibrary.mockResolvedValueOnce(lib(["A"]));
    render(<App />);
    await screen.findByText("Paper A");
    expect(screen.queryByText(/new paper/)).not.toBeInTheDocument();

    // A second unclassified paper appears on the next refresh → banner.
    getLibrary.mockResolvedValueOnce(lib(["A", "B"]));
    fireEvent.click(screen.getByRole("button", { name: "Refresh library" }));
    await waitFor(() =>
      expect(screen.getByText(/1 new paper/)).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() =>
      expect(screen.queryByText(/1 new paper/)).not.toBeInTheDocument(),
    );
  });
});
