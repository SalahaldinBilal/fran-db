// Run with: npm run version:bump <version>  (e.g. 0.2.0)
// Sets the version in package.json, Cargo.toml and tauri.conf.json, and turns CHANGELOG.md's
// "Unreleased" section into a dated section for that version, which deploy.ts uses as the release notes.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const newVersion = process.argv[2];

if (!newVersion || !/^\d+\.\d+\.\d+$/.test(newVersion)) {
  console.error("Usage: npm run version:bump <version>  (e.g. 0.2.0)");
  process.exit(1);
}

function replaceOrThrow(contents: string, pattern: RegExp, replacement: string, filePath: string): string {
  if (!pattern.test(contents)) {
    throw new Error(`Could not find a version field matching ${pattern} in ${filePath}`);
  }
  return contents.replace(pattern, replacement);
}

function bumpJsonVersion(relativePath: string) {
  const filePath = fileURLToPath(new URL(relativePath, import.meta.url));
  const contents = readFileSync(filePath, "utf-8");
  writeFileSync(filePath, replaceOrThrow(contents, /"version":\s*"[^"]*"/, `"version": "${newVersion}"`, filePath));
  console.log(`Updated ${relativePath}`);
}

function bumpCargoVersion(relativePath: string) {
  const filePath = fileURLToPath(new URL(relativePath, import.meta.url));
  const contents = readFileSync(filePath, "utf-8");
  // Anchored to a line start so dependency versions like `{ version = "..." }` are left alone.
  writeFileSync(filePath, replaceOrThrow(contents, /^version = "[^"]*"/m, `version = "${newVersion}"`, filePath));
  console.log(`Updated ${relativePath}`);
}

function cutChangelog() {
  const relativePath = "../CHANGELOG.md";
  const filePath = fileURLToPath(new URL(relativePath, import.meta.url));
  const contents = readFileSync(filePath, "utf-8");

  const unreleasedHeading = "## [Unreleased]";
  const headingStart = contents.indexOf(unreleasedHeading);
  if (headingStart === -1) {
    throw new Error(`Could not find "${unreleasedHeading}" in ${relativePath}`);
  }

  const sectionStart = headingStart + unreleasedHeading.length;
  const nextHeadingIndex = contents.indexOf("\n## [", sectionStart);
  const sectionEnd = nextHeadingIndex === -1 ? contents.length : nextHeadingIndex;
  const sectionBody = contents.slice(sectionStart, sectionEnd).trim();

  if (!sectionBody) {
    console.warn(`Warning: the "${unreleasedHeading}" section in ${relativePath} is empty, so this release has no notes.`);
  }

  const date = new Date().toISOString().slice(0, 10);
  const releasedHeading = `## [${newVersion}] - ${date}`;
  const replacement = sectionBody
    ? `${unreleasedHeading}\n\n${releasedHeading}\n\n${sectionBody}\n`
    : `${unreleasedHeading}\n\n${releasedHeading}\n`;

  writeFileSync(filePath, contents.slice(0, headingStart) + replacement + contents.slice(sectionEnd));
  console.log(`Updated ${relativePath}`);
}

bumpJsonVersion("../package.json");
bumpCargoVersion("../src-tauri/Cargo.toml");
bumpJsonVersion("../src-tauri/tauri.conf.json");
cutChangelog();

console.log(`\nBumped to ${newVersion}. Cargo.lock picks it up on the next cargo check or build.`);
