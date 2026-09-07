import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

if (process.argv.length !== 2) throw new Error("usage: node scripts/stage-marketplace-site.mjs");

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const source = join(root, "web/marketplace");
const output = join(root, "published/marketplace");
mkdirSync(join(output, "assets"), { recursive: true });
let html = readFileSync(join(source, "index.html"), "utf8");
const current = new Set();
for (const [name, path] of [["app.js", "app.js"], ["styles.css", "styles.css"], ["catalog-policy.js", "policies/production.js"]]) {
  const bytes = readFileSync(join(source, path));
  const hash = createHash("sha256").update(bytes).digest("hex").slice(0, 16);
  const filename = name.replace(/(\.[^.]+)$/u, `-${hash}$1`);
  current.add(filename);
  writeFileSync(join(output, filename), bytes);
  html = html.replace(`./${name}`, `./${filename}`);
}
for (const entry of readdirSync(join(source, "assets"), { withFileTypes: true })) {
  if (!entry.isFile()) throw new Error("marketplace assets must be regular files");
  copyFileSync(join(source, "assets", entry.name), join(output, "assets", entry.name));
}
writeFileSync(join(output, "index.html"), html);
// Only the site shell is replaced; signed catalog and immutable objects are untouched.
for (const name of readdirSync(output)) {
  if (/^(?:app(?:-[a-f0-9]{16})?\.js|styles(?:-[a-f0-9]{16})?\.css|catalog-policy(?:-[a-f0-9]{16})?\.js)$/u.test(name) && !current.has(name)) {
    unlinkSync(join(output, name));
  }
}
console.log("Marketplace UI staged in published/marketplace; signed catalog and packages unchanged.");
