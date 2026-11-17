#!/usr/bin/env bun
// Add the Windows entry to the latest.json of a release, which the in-app updater reads.
// scripts/release.sh writes the file with the macOS entry. This script adds the entry for
// the setup exe that scripts/build-windows.sh --installer made and signed.
//
//   bun scripts/latest-json-windows.mjs <latest.json> <MarginNotes-setup-<version>.exe.sig>
//
// The key is `windows-x86_64`. tauri-plugin-updater looks for `windows-x86_64-nsis` first
// and then for `windows-x86_64`, so one entry serves the copy that the installer
// installed. The other entries and fields stay as they are.
// MARGIN_UPDATE_BASE_URL replaces the GitHub download folder, for tests.
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const [manifestPath, sigPath] = process.argv.slice(2);
if (!manifestPath || !sigPath) {
  console.error("Usage: bun scripts/latest-json-windows.mjs <latest.json> <setup.exe.sig>");
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const version = manifest.version;
const setup = basename(sigPath).replace(/\.sig$/, "");
// The setup exe must be the one for the version in latest.json.
if (setup !== `MarginNotes-setup-${version}.exe`) {
  console.error(`${setup} is not the setup for version ${version} in ${manifestPath}.`);
  process.exit(1);
}
const signature = readFileSync(sigPath, "utf8").trim();
if (!signature) {
  console.error(`${sigPath} is empty.`);
  process.exit(1);
}

const base = process.env.MARGIN_UPDATE_BASE_URL || `https://github.com/askysh/margin/releases/download/v${version}`;
manifest.platforms = manifest.platforms || {};
manifest.platforms["windows-x86_64"] = { signature, url: `${base}/${setup}` };
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
console.log(`Added windows-x86_64 to ${manifestPath}: ${base}/${setup}`);
