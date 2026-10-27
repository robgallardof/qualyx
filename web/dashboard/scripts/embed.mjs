// Copy the static export into qualyx-api so `cargo build` embeds it without needing Node.
import { cpSync, existsSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "..", "out");
const target = join(here, "..", "..", "..", "crates", "qualyx-api", "dashboard");

if (!existsSync(join(out, "index.html"))) {
  console.error("embed: out/index.html is missing; run `next build` first");
  process.exit(1);
}
rmSync(target, { recursive: true, force: true });
cpSync(out, target, { recursive: true });
console.log(`embed: copied ${out} -> ${target}`);
