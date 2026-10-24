import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: node scripts/set-version.mjs <major.minor.patch>");
  process.exit(1);
}

for (const [file, update] of [
  ["package.json", (json) => { json.version = version; }],
  ["crates/livtet-desktop/tauri.conf.json", (json) => { json.version = version; }],
]) {
  const json = JSON.parse(readFileSync(file, "utf8"));
  update(json);
  writeFileSync(file, `${JSON.stringify(json, null, 2)}\n`);
}

console.log(`set version ${version}`);
