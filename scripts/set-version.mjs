// Sets the app version everywhere it lives: pnpm version:set 0.2.0
import { readFileSync, writeFileSync } from "node:fs";

const v = process.argv[2]?.replace(/^v/, "");
if (!v || !/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(v)) {
  console.error("Usage: pnpm version:set <x.y.z>");
  process.exit(1);
}

for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(readFileSync(file, "utf8"));
  json.version = v;
  writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
}

const cargo = "src-tauri/Cargo.toml";
writeFileSync(cargo, readFileSync(cargo, "utf8").replace(/^version = ".*"$/m, `version = "${v}"`));

const lock = "src-tauri/Cargo.lock";
writeFileSync(
  lock,
  readFileSync(lock, "utf8").replace(/(\[\[package\]\]\nname = "casylum"\nversion = )".*"/, `$1"${v}"`),
);

console.log(`Version set to ${v}. Next: commit, then git tag v${v} && git push origin v${v}`);
