import test from "node:test";
import assert from "node:assert/strict";
import { lstat, readdir, readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const publicUrl = process.argv[2]
  ? pathToFileURL(`${resolve(process.argv[2])}/`)
  : new URL("../../web/landing/", import.meta.url);
const html = await readFile(new URL("index.html", publicUrl), "utf8");
const css = await readFile(new URL("styles-5428c51a.css", publicUrl), "utf8");
const js = await readFile(new URL("site-7ef11cd4.js", publicUrl), "utf8");

test("every CSS class is referenced by the page or its progressive enhancements", () => {
  const referencedClasses = new Set();

  for (const match of html.matchAll(/class="([^"]+)"/g)) {
    for (const className of match[1].split(/\s+/)) referencedClasses.add(className);
  }

  for (const match of js.matchAll(/classList\.(?:add|remove|toggle)\("([^"]+)"/g)) {
    referencedClasses.add(match[1]);
  }

  const cssClasses = new Set(
    [...css.matchAll(/(?<![\w/-])\.([a-zA-Z_-][\w-]*)/g)].map(
      (match) => match[1],
    ),
  );
  const deadClasses = [...cssClasses]
    .filter((className) => !referencedClasses.has(className))
    .sort();

  assert.deepEqual(deadClasses, []);
});

test("every shipped binary asset is referenced by the landing", async () => {
  const source = `${html}\n${css}\n${js}`;
  const files = await readdir(publicUrl, { recursive: true });
  const binaryAssets = files.filter(
    (file) =>
      !file.startsWith("marketplace/") &&
      /\.(?:jpe?g|png|svg|webp|woff2)$/i.test(file),
  );
  const unusedAssets = binaryAssets
    .filter((file) => !source.includes(file))
    .sort();

  assert.deepEqual(unusedAssets, []);
});

test("repeated voice-state icons share one SVG definition", () => {
  assert.match(html, /<symbol id="icon-mic-off"/);
  assert.match(html, /<symbol id="icon-headphone-off"/);
  assert.equal((html.match(/href="#icon-mic-off"/g) ?? []).length, 2);
  assert.equal((html.match(/href="#icon-headphone-off"/g) ?? []).length, 2);
  assert.equal((html.match(/<path d="M12 19v3"/g) ?? []).length, 1);
  assert.equal((html.match(/<path d="M21 14h-1\.343"/g) ?? []).length, 1);
});

test("the static tree contains only safe local regular assets", async () => {
  const files = await readdir(publicUrl, { recursive: true });
  for (const relative of files) {
    assert.match(relative, /^(?!\/)(?!.*(?:^|\/)\.\.?(?:\/|$))[A-Za-z0-9._/-]+$/u);
    const metadata = await lstat(new URL(relative, publicUrl));
    assert.equal(metadata.isSymbolicLink(), false, `symlink: ${relative}`);
    assert.equal(
      metadata.isDirectory() || metadata.isFile(),
      true,
      `special file: ${relative}`,
    );
  }

  for (const relative of files.filter((file) => file.endsWith(".html"))) {
    const document = await readFile(new URL(relative, publicUrl), "utf8");
    assert.doesNotMatch(document, /<script\b[^>]*src=["'](?:https?:)?\/\//iu);
    assert.doesNotMatch(
      document,
      /<link\b(?=[^>]*rel=["'](?:stylesheet|modulepreload|preload)["'])[^>]*href=["'](?:https?:)?\/\//iu,
    );
    assert.doesNotMatch(document, /<script\b(?![^>]*\bsrc=)[^>]*>/iu);
    assert.doesNotMatch(document, /<style\b|\sstyle=["']/iu);
    assert.doesNotMatch(document, /(?:src|href)=["'][^"']*(?:^|\/)\.\.(?:\/|$)/imu);
    assert.doesNotMatch(document, /Development\s*[—-]\s*unverified/iu);
  }
});
