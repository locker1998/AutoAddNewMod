import { copyFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src-tauri", "target", "release", "AutoAddNewMod.exe");
const dest = join(root, "AutoAddNewMod.exe");

if (!existsSync(src)) {
  console.warn("Release exe not found, skip copy:", src);
  process.exit(0);
}

copyFileSync(src, dest);
console.log("Copied release exe to", dest);
