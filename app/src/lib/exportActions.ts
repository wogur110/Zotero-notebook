// Side-effectful counterparts to the pure Markdown builder: save via the
// native dialog, or copy to the clipboard. Kept out of export.ts so that
// stays unit-testable without Tauri.

import { save } from "@tauri-apps/plugin-dialog";
import * as api from "../api";

/** Show a save dialog and write `content`; returns the path, or null if cancelled. */
export async function saveMarkdown(
  defaultName: string,
  content: string,
): Promise<string | null> {
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (typeof path !== "string") return null;
  await api.writeTextFile(path, content);
  return path;
}

export async function copyText(content: string): Promise<void> {
  await navigator.clipboard.writeText(content);
}
