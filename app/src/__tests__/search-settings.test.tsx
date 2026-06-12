import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, Item, Library } from "../types";

let savedKeys: Record<string, boolean> = {};

vi.mock("../api", () => ({
  hasApiKey: vi.fn(async (p: string) => savedKeys[p] ?? false),
  saveApiKey: vi.fn(async (p: string) => {
    savedKeys[p] = true;
  }),
  deleteApiKey: vi.fn(async (p: string) => {
    savedKeys[p] = false;
  }),
  testApiKey: vi.fn(async () => {
    throw "Invalid Anthropic API key";
  }),
  exportPluginXpi: vi.fn(async (dir: string) => `${dir}/zotero-notebook.xpi`),
  errorMessage: (e: unknown) => String(e),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "/tmp/x"),
}));

import * as api from "../api";
import SearchPalette from "../components/SearchPalette";
import SettingsView from "../views/SettingsView";
import OnboardingView from "../views/OnboardingView";

const item = (key: string, title: string, tags: string[] = []): Item => ({
  key,
  title,
  itemType: "journalArticle",
  creators: ["Ada Lovelace"],
  year: 2021,
  publication: "Venue",
  doi: null,
  url: null,
  abstractText: null,
  tags,
  dateAdded: "2026-01-01T00:00:00Z",
  collectionKeys: [],
  attachment: null,
});

const library: Library = {
  collections: [],
  items: [
    item("I1", "Diffusion models for images", ["diffusion"]),
    item("I2", "Transformers for language"),
  ],
  writable: true,
};

const settings: AppSettings = {
  defaultProvider: "gemini",
  geminiModel: "gemini-2.5-pro",
  anthropicModel: "claude-opus-4-8",
  localBaseUrl: "http://127.0.0.1:11434/v1",
  localModel: "llama3.1:8b",
  zoteroBaseUrl: "http://127.0.0.1:23119",
  fileRoot: null,
};

beforeEach(() => {
  savedKeys = {};
  vi.clearAllMocks();
});

describe("SearchPalette", () => {
  it("filters by query and opens on Enter", async () => {
    const onOpen = vi.fn();
    render(
      <SearchPalette
        open
        library={library}
        summaries={[]}
        onClose={() => {}}
        onOpenItem={onOpen}
      />,
    );
    const input = screen.getByLabelText("Search query");
    fireEvent.change(input, { target: { value: "diffusion" } });
    await screen.findByText("Diffusion models for images");
    expect(screen.queryByText("Transformers for language")).not.toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith("I1");
  });

  it("matches papers by their AI summary text", async () => {
    const onOpen = vi.fn();
    render(
      <SearchPalette
        open
        library={library}
        summaries={[
          {
            itemKey: "I2",
            summary:
              "Introduces wavelet-based attention for long documents.",
            provider: "gemini",
            model: "gemini-2.5-pro",
            createdAt: "2026-06-11T00:00:00Z",
            source: "abstract",
          },
        ]}
        onClose={() => {}}
        onOpenItem={onOpen}
      />,
    );
    // "wavelet" appears ONLY in the stored summary, not in any metadata.
    fireEvent.change(screen.getByLabelText("Search query"), {
      target: { value: "wavelet" },
    });
    await screen.findByText("Transformers for language");
    expect(
      screen.queryByText("Diffusion models for images"),
    ).not.toBeInTheDocument();
  });

  it("shows recent items when the query is empty", () => {
    render(
      <SearchPalette
        open
        library={library}
        summaries={[]}
        onClose={() => {}}
        onOpenItem={() => {}}
      />,
    );
    expect(screen.getByText("Recent")).toBeInTheDocument();
    expect(screen.getByText("Transformers for language")).toBeInTheDocument();
  });
});

describe("SettingsView", () => {
  it("saves an API key and flips the state chip; failed test shows the error", async () => {
    render(
      <SettingsView
        settings={settings}
        status={null}
        onSave={async () => {}}
        onClose={() => {}}
      />,
    );

    const chips = await screen.findAllByText("No key");
    expect(chips.length).toBe(2);

    const keyInput = screen.getByLabelText("Anthropic API key");
    fireEvent.change(keyInput, { target: { value: "sk-ant-xxx" } });
    const saveButtons = screen.getAllByRole("button", { name: /Save key/ });
    fireEvent.click(saveButtons[1]); // anthropic block is second
    await waitFor(() =>
      expect(api.saveApiKey).toHaveBeenCalledWith("anthropic", "sk-ant-xxx"),
    );
    await screen.findByText("Key saved");

    // Test fails with the provider's message.
    const testButtons = screen.getAllByRole("button", { name: "Test" });
    fireEvent.click(testButtons[1]);
    await screen.findByText("Invalid Anthropic API key");
  });

  it("exports the plugin xpi via the directory picker", async () => {
    render(
      <SettingsView
        settings={settings}
        status={null}
        onSave={async () => {}}
        onClose={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Save plugin file/ }));
    await screen.findByText("/tmp/x/zotero-notebook.xpi");
    expect(api.exportPluginXpi).toHaveBeenCalledWith("/tmp/x");
  });
});

describe("OnboardingView", () => {
  it("reflects status and fires onDone", async () => {
    const onDone = vi.fn();
    render(
      <OnboardingView
        status={{
          running: true,
          pluginInstalled: false,
          pluginVersion: null,
          hint: null,
        }}
        onRefreshStatus={() => {}}
        onDone={onDone}
      />,
    );
    expect(
      screen.getByText("Connected to Zotero on this computer."),
    ).toBeInTheDocument();
    expect(screen.getByText(/Install it later from Settings/)).toBeInTheDocument();
    await screen.findByText(/add a Gemini or Anthropic key/);

    fireEvent.click(screen.getByRole("button", { name: "Open my library" }));
    expect(onDone).toHaveBeenCalled();
  });
});
