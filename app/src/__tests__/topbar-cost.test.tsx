import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import Topbar from "../components/Topbar";
import type { UsageSummary, ZoteroStatus } from "../types";

const status: ZoteroStatus = {
  running: true,
  pluginInstalled: true,
  pluginVersion: "1.3.0",
  hint: null,
};

const noop = () => {};

const render_ = (usage: UsageSummary | null) =>
  render(
    <Topbar
      status={status}
      usage={usage}
      refreshing={false}
      view="library"
      onRefresh={noop}
      onOpenSearch={noop}
      onToggleSettings={noop}
    />,
  );

describe("Topbar cost pill", () => {
  it("is hidden until at least one operation has run", () => {
    render_({
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCostUsd: 0,
      operationCount: 0,
    });
    expect(screen.queryByText(/\$/)).not.toBeInTheDocument();
  });

  it("shows the estimated cumulative cost", () => {
    render_({
      totalInputTokens: 1000,
      totalOutputTokens: 200,
      totalCostUsd: 0.42,
      operationCount: 3,
    });
    expect(screen.getByText("$0.42")).toBeInTheDocument();
  });
});
