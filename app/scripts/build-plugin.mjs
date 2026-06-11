// Packages the Zotero companion plugin into an .xpi (a plain ZIP with the
// plugin files at the archive root) and drops it where Tauri bundles it as
// a resource. Run from app/: `npm run build:plugin`.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import AdmZip from "adm-zip";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");
const pluginDir = join(repoRoot, "zotero-plugin");
const outDir = join(repoRoot, "app", "src-tauri", "resources");
const outFile = join(outDir, "zotero-notebook.xpi");

const include = ["manifest.json", "bootstrap.js", "prefs.js"];

const manifest = JSON.parse(
  readFileSync(join(pluginDir, "manifest.json"), "utf8"),
);

mkdirSync(outDir, { recursive: true });

const zip = new AdmZip();
let added = 0;
for (const name of include) {
  const path = join(pluginDir, name);
  if (!existsSync(path)) continue;
  zip.addLocalFile(path); // at the ZIP root — Zotero requires this layout
  added += 1;
}
if (added < 2) {
  console.error("build-plugin: expected at least manifest.json and bootstrap.js");
  process.exit(1);
}
zip.writeZip(outFile);
console.log(`built ${outFile} (v${manifest.version}, ${added} files)`);
