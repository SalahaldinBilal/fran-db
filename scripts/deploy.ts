// Run with: node scripts/deploy.ts  (in the release workflow, after every platform's build)
// Expects each platform's `src-tauri/target/.../bundle` folder under bundles/bundle-<platform>/, and a
// `## [<version>]` section in CHANGELOG.md, which becomes the release notes.
// Required env vars: B2_APPLICATION_KEY_ID, B2_APPLICATION_KEY, GH_TOKEN.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const B2_BUCKET = "frandb-files";
const DOWNLOAD_DOMAIN = "frandb.won.fyi";

const rootDir = fileURLToPath(new URL("..", import.meta.url));
const bundlesDir = join(rootDir, "bundles");
const stagingDir = join(rootDir, "release-files");

type Artifact = {
  label: string,
  bundle: string,
  folder: string,
  suffix: string,
  publishedSuffix: string,
  installer: boolean,
  updaterPlatforms: string[],
};

const ARTIFACTS: Artifact[] = [
  {
    label: "Windows",
    bundle: "bundle-windows",
    folder: "nsis",
    suffix: "-setup.exe",
    publishedSuffix: "x64-setup.exe",
    installer: true,
    updaterPlatforms: ["windows-x86_64-nsis", "windows-x86_64"],
  },
  {
    label: "macOS (Apple Silicon and Intel)",
    bundle: "bundle-macos",
    folder: "dmg",
    suffix: ".dmg",
    publishedSuffix: "universal.dmg",
    installer: true,
    updaterPlatforms: [],
  },
  {
    label: "macOS update",
    bundle: "bundle-macos",
    folder: "macos",
    suffix: ".app.tar.gz",
    publishedSuffix: "universal.app.tar.gz",
    installer: false,
    updaterPlatforms: ["darwin-aarch64-app", "darwin-x86_64-app", "darwin-aarch64", "darwin-x86_64"],
  },
  {
    label: "Linux AppImage",
    bundle: "bundle-linux",
    folder: "appimage",
    suffix: ".AppImage",
    publishedSuffix: "amd64.AppImage",
    installer: true,
    updaterPlatforms: ["linux-x86_64-appimage", "linux-x86_64"],
  },
];

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required env var: ${name}`);
  return value;
}

function b2(...args: string[]) {
  execFileSync("b2", args, { stdio: "inherit" });
}

function gh(...args: string[]) {
  execFileSync("gh", args, { stdio: "inherit" });
}

function upload(localPath: string, remotePath: string) {
  console.log(`Uploading ${localPath} -> b2://${B2_BUCKET}/${remotePath}`);
  b2("file", "upload", B2_BUCKET, localPath, remotePath);
}

function downloadUrl(remotePath: string): string {
  return `https://${DOWNLOAD_DOMAIN}/${remotePath}`;
}

function readChangelogNotes(version: string): string {
  const contents = readFileSync(join(rootDir, "CHANGELOG.md"), "utf-8");
  const heading = `## [${version}]`;
  const headingStart = contents.indexOf(heading);
  if (headingStart === -1) {
    throw new Error(`Could not find a "${heading}" section in CHANGELOG.md, did you run \`npm run version:bump\` before tagging?`);
  }

  const sectionStart = contents.indexOf("\n", headingStart) + 1;
  const nextHeadingIndex = contents.indexOf("\n## [", sectionStart);
  const sectionEnd = nextHeadingIndex === -1 ? contents.length : nextHeadingIndex;
  return contents.slice(sectionStart, sectionEnd).trim();
}

// Tauri names files after the product name, which has a space, so they're found by suffix.
function findBuiltFile(artifact: Artifact): string {
  const dir = join(bundlesDir, artifact.bundle, artifact.folder);
  const files = existsSync(dir) ? readdirSync(dir) : [];
  const match = files.find(file => file.endsWith(artifact.suffix));
  if (!match) {
    throw new Error(`No *${artifact.suffix} file in ${dir} (found: ${files.join(", ") || "nothing"})`);
  }
  return join(dir, match);
}

const { name: slug } = JSON.parse(readFileSync(join(rootDir, "package.json"), "utf-8"));
const { version } = JSON.parse(readFileSync(join(rootDir, "src-tauri", "tauri.conf.json"), "utf-8"));
const tag = `v${version}`;

const b2KeyId = requireEnv("B2_APPLICATION_KEY_ID");
const b2AppKey = requireEnv("B2_APPLICATION_KEY");
const notes = readChangelogNotes(version);

rmSync(stagingDir, { recursive: true, force: true });
mkdirSync(stagingDir, { recursive: true });

const files = ARTIFACTS.map(artifact => {
  const builtPath = findBuiltFile(artifact);
  const name = `${slug}_${version}_${artifact.publishedSuffix}`;
  const stagedPath = join(stagingDir, name);
  copyFileSync(builtPath, stagedPath);

  const signaturePath = `${builtPath}.sig`;
  const hasSignature = existsSync(signaturePath);
  if (artifact.updaterPlatforms.length > 0 && !hasSignature) {
    throw new Error(`${builtPath} has no .sig next to it, was TAURI_SIGNING_PRIVATE_KEY set for the build?`);
  }

  return {
    artifact,
    name,
    stagedPath,
    remotePath: `${artifact.installer ? "installer" : "updater"}/${version}/${name}`,
    latestPath: `${slug}_latest_${artifact.publishedSuffix}`,
    signature: hasSignature ? readFileSync(signaturePath, "utf-8") : null,
  };
});

b2("account", "authorize", b2KeyId, b2AppKey);

for (const file of files) {
  upload(file.stagedPath, file.remotePath);
}

const installers = files.filter(file => file.artifact.installer);
const downloadLines = installers.map(file => `- ${file.artifact.label}: ${downloadUrl(file.remotePath)}`);
const releaseNotes = `${notes}\n\n### Downloads\n\n${downloadLines.join("\n")}`;
gh("release", "create", tag, "--title", tag, "--notes", releaseNotes, ...installers.map(file => file.stagedPath));

for (const file of installers) {
  upload(file.stagedPath, file.latestPath);
}

const platforms: Record<string, { signature: string, url: string }> = {};
for (const file of files) {
  if (!file.signature) continue;
  for (const platform of file.artifact.updaterPlatforms) {
    platforms[platform] = { signature: file.signature, url: downloadUrl(file.remotePath) };
  }
}

const manifest = {
  version: tag,
  notes,
  pub_date: new Date().toISOString(),
  platforms,
};
const manifestPath = join(stagingDir, "latest.json");
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
upload(manifestPath, "updater/latest.json");

console.log(`\nDeployed ${tag}.`);
