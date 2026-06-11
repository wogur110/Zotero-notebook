import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Tauri's IPC is not available under jsdom; individual tests mock
// `@tauri-apps/api/core` / our `api.ts` wrappers as needed.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => {
    throw new Error("invoke() must be mocked in tests");
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
