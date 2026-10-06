// Asserts that the app version agrees across the three files that declare it.
// Tauri warns (and the installer name drifts) when `tauri.conf.json` and
// `package.json` disagree, so this runs as the first step of `npm run build`.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(relative) {
  return JSON.parse(readFileSync(join(root, relative), "utf8"));
}

function readCargoVersion(relative) {
  const contents = readFileSync(join(root, relative), "utf8");
  // Anchor on the [package] section so dependency versions can never match.
  const section = contents
    .split(/^\[/m)
    .find((part) => part.startsWith("package]"));
  const match = section?.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error(`could not read [package] version from ${relative}`);
  }
  return match[1];
}

const sources = {
  "package.json": readJson("package.json").version,
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "src-tauri/Cargo.toml": readCargoVersion("src-tauri/Cargo.toml"),
};

const versions = new Set(Object.values(sources));
if (versions.size > 1) {
  console.error("version mismatch between:");
  for (const [file, version] of Object.entries(sources)) {
    console.error(`  ${version}\t${file}`);
  }
  process.exit(1);
}

console.log(`versions agree: ${[...versions][0]}`);
