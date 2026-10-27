// Copy the static export into qualyx-api so `cargo build` embeds it without needing Node.
// Webpack chunk hashes differ between operating systems, so drift is tracked by a hash of the
// sources (line endings normalized) stored beside the export: `--check` compares it.
import { createHash } from "node:crypto";
import { cpSync, existsSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(app, "out");
const target = join(app, "..", "..", "crates", "qualyx-api", "dashboard");
const stamp = join(target, ".source-sha256");
const inputs = ["components.json", "next.config.ts", "package-lock.json", "package.json", "postcss.config.mjs", "tsconfig.json", "src"];

function files(path) {
  return statSync(path).isDirectory() ? readdirSync(path).sort().flatMap((name) => files(join(path, name))) : [path];
}

export function sourceHash() {
  const hash = createHash("sha256");
  for (const file of inputs.flatMap((input) => files(join(app, input)))) {
    hash.update(relative(app, file).replaceAll("\\", "/") + "\0");
    hash.update(readFileSync(file, "utf8").replaceAll("\r\n", "\n") + "\0");
  }
  return hash.digest("hex");
}

if (process.argv.includes("--check")) {
  const committed = existsSync(stamp) ? readFileSync(stamp, "utf8").trim() : "(missing)";
  if (committed !== sourceHash()) {
    console.error(`embed: crates/qualyx-api/dashboard is stale (${committed}); run \`npm run build\` and commit it`);
    process.exit(1);
  }
  console.log("embed: committed export matches the sources");
} else {
  if (!existsSync(join(out, "index.html"))) {
    console.error("embed: out/index.html is missing; run `next build` first");
    process.exit(1);
  }
  rmSync(target, { recursive: true, force: true });
  cpSync(out, target, { recursive: true });
  writeFileSync(stamp, sourceHash() + "\n");
  console.log(`embed: copied ${out} -> ${target}`);
}
