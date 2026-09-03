"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const vm = require("node:vm");

const DEVELOPMENT_BASE = "http://127.0.0.1:8787/marketplace/v1/";
const PRODUCTION_BASE = "https://overcrow.playervox.com/marketplace/v1/";
const DIGEST = "a".repeat(64);
const SIXTEEN_LOCALES = [
  "en", "fr", "de", "es", "it", "pt", "nl", "sv",
  "da", "no", "fi", "pl", "cs", "hu", "ro", "el",
];

class Element {
  constructor(tag = "div") {
    this.tagName = tag;
    this.children = [];
    this.listeners = new Map();
    this.attributes = new Map();
    this._textContent = "";
    this.value = "en";
  }

  set textContent(value) {
    this._textContent = value;
    this.children = [];
  }

  get textContent() {
    return this._textContent;
  }

  append(...items) {
    this.children.push(...items);
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  dispatch(type) {
    this.listeners.get(type)();
  }

  setAttribute(name, value) {
    assert.equal(typeof name, "string");
    assert.equal(typeof value, "string");
    this.attributes.set(name, value);
  }

  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }
}

function webTarget(overrides = {}) {
  const id = overrides.id || "com.playervox.overcrow.warframe.market";
  const version = overrides.version || "2.0.0";
  const sha256 = overrides.packageSha256 || DIGEST;
  const manifest = {
    schemaVersion: 1,
    id,
    version,
    apiVersion: "1",
    entrypoints: { view: "index.html", controller: "controller.html" },
    permissions: {
      network: [{
        origin: "https://api.warframe.market",
        method: "GET",
        pathPrefix: "/v2/",
      }],
      storage: true,
      clipboardWrite: true,
    },
    files: {
      "index.html": { sha256: DIGEST, bytes: 757 },
      "controller.html": { sha256: DIGEST, bytes: 205 },
    },
    ...overrides.manifest,
  };
  const listing = {
    author: "PlayerVox",
    spdxLicense: "AGPL-3.0-only",
    sourceUrl: "https://github.com/PlayerVox/playervox-overcrow-marketplace",
    defaultLocale: "en",
    localizations: [
      {
        locale: "en",
        name: "Warframe Market",
        description: "Searches public PC items from a versioned IndexedDB catalog.",
      },
      {
        locale: "fr",
        name: "Marché Warframe",
        description: "Recherche les objets publics PC depuis un catalogue IndexedDB.",
      },
    ],
    ...overrides.listing,
  };
  return {
    manifest,
    listing,
    packageUrl: `${overrides.base || DEVELOPMENT_BASE}packages/${id}/${version}/${sha256}.ocpkg`,
    packageSize: 4096,
    packageSha256: sha256,
    status: "verified",
    ...overrides.target,
  };
}

function envelope(targets, keyId = "overcrow-development-2026") {
  const payload = {
    schemaVersion: 1,
    sequence: 1,
    generatedAt: "2026-01-01T00:00:00Z",
    expiresAt: "2026-04-01T00:00:00Z",
    targets,
  };
  return JSON.stringify({
    schemaVersion: 1,
    keyId,
    payload: Buffer.from(JSON.stringify(payload)).toString("base64url"),
    signature: "b".repeat(86),
  });
}

function generated() {
  return envelope([webTarget()]);
}

function payload(body) {
  const parsed = JSON.parse(body);
  return JSON.parse(Buffer.from(parsed.payload, "base64url"));
}

function withTargets(change, source = generated()) {
  const parsed = JSON.parse(source);
  const body = JSON.parse(Buffer.from(parsed.payload, "base64url"));
  parsed.payload = Buffer.from(JSON.stringify({
    ...body,
    targets: change(body.targets),
  })).toString("base64url");
  return JSON.stringify(parsed);
}

function productionCatalog() {
  return envelope([webTarget({
    base: PRODUCTION_BASE,
  })], "overcrow-production-2026-01");
}

function streamed(body, options) {
  const chunks = options.chunks || (options.malformedChunk
    ? ["not bytes"]
    : [new Uint8Array(Buffer.from(body))]);
  let index = 0;
  return {
    getReader: () => ({
      read: async () => index < chunks.length
        ? { done: false, value: chunks[index++] }
        : { done: true },
    }),
  };
}

async function run(body, options = {}) {
  const catalog = new Element();
  const language = new Element("select");
  const trust = new Element("p");
  const elements = new Map([
    ["catalog", catalog],
    ["language", language],
    ["trust-label", trust],
  ]);
  const document = {
    createElement: (tag) => new Element(tag),
    getElementById: (id) => elements.get(id),
    documentElement: { lang: "en" },
  };
  const requests = [];
  const context = {
    document,
    TextDecoder,
    Uint8Array,
    URL,
    atob: (value) => Buffer.from(value, "base64").toString("binary"),
    fetch: async (url) => {
      requests.push(url);
      return {
        ok: options.ok ?? true,
        headers: { get: () => options.contentLength },
        body: options.noBody ? undefined : streamed(body, options),
      };
    },
  };
  if (options.policyObject) {
    context.overcrowMarketplacePolicy = options.policyObject;
  } else {
    const policyPath = options.policy || "web/marketplace/policies/development.js";
    vm.runInNewContext(fs.readFileSync(policyPath, "utf8"), context);
  }
  vm.runInNewContext(fs.readFileSync("web/marketplace/app.js", "utf8"), context);
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  return { catalog, language, trust, document, requests };
}

function cardText(card) {
  if (!card) return "";
  return [card.textContent, ...card.children.map(cardText)].filter(Boolean).join("\n");
}

function descendants(element) {
  return element.children.flatMap((child) => [child, ...descendants(child)]);
}

function card(page, pattern) {
  return page.catalog.children.find((item) => pattern.test(cardText(item)));
}

function unavailable(page) {
  assert.equal(page.catalog.children.length, 1);
  assert.equal(page.catalog.children[0].textContent, "Catalog unavailable.");
}

test("development mode renders a Web API v1 catalog card", async () => {
  const body = generated();
  assert.equal(payload(body).targets.length, 1);
  const page = await run(body);
  const index = fs.readFileSync("web/marketplace/index.html", "utf8");
  assert.match(index, /<option value="en" selected>English<\/option>/u);
  assert.match(index, /<option value="fr">Français<\/option>/u);
  assert.deepEqual(page.requests, ["/marketplace/v1/catalog.json"]);
  assert.equal(page.trust.textContent, "Development — unverified");
  assert.equal(page.catalog.children.length, 1);
  const market = cardText(card(page, /Warframe Market/u));
  for (const expected of [
    /Version 2\.0\.0/u,
    /Author PlayerVox/u,
    /api\.warframe\.market/u,
    /Stores private widget data/u,
    /Writes to clipboard on request/u,
  ]) assert.match(market, expected);
  assert.doesNotMatch(market, /provider/iu);
  assert.doesNotMatch(market, /component\.wasm/u);
});

test("production mode renders complete catalog metadata without a development claim", async () => {
  const page = await run(productionCatalog(), {
    policy: "web/marketplace/policies/production.js",
  });
  assert.equal(
    page.trust.textContent,
    "Production catalog — installs are verified by OverCrow",
  );
  const market = cardText(card(page, /Warframe Market/u));
  for (const expected of [
    "Version 2.0.0",
    "Author PlayerVox",
    "License AGPL-3.0-only",
    "Languages en, fr",
    "Verified catalog entry",
  ]) assert.match(market, new RegExp(expected, "u"));
  assert.doesNotMatch(market, /Development — unverified/u);

  const source = descendants(card(page, /Warframe Market/u))
    .find((element) => element.tagName === "a");
  assert.equal(
    source.getAttribute("href"),
    "https://github.com/PlayerVox/playervox-overcrow-marketplace",
  );
  assert.equal(source.getAttribute("rel"), "noreferrer noopener");
});

test("creator strings are assigned only through textContent", async () => {
  const poisoned = withTargets((targets) => {
    targets[0].listing.localizations[0].name = "<img src=x onerror=globalThis.pwned=true>";
    return targets;
  });
  const page = await run(poisoned);
  const item = card(page, /<img src=x/u);
  assert.ok(item);
  assert.equal(item.children.some((child) => child.tagName === "script"), false);
  assert.equal(descendants(item).some((child) => child.tagName === "img"), false);
  assert.match(cardText(item), /<img src=x onerror=globalThis\.pwned=true>/u);
});

test("French UI falls back to English creator copy when only English is supplied", async () => {
  const englishOnly = withTargets((targets) => {
    targets[0].listing.defaultLocale = "en";
    targets[0].listing.localizations = targets[0].listing.localizations.filter(
      (entry) => entry.locale === "en",
    );
    return targets;
  });
  const page = await run(englishOnly);
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.equal(page.document.documentElement.lang, "fr");
  assert.equal(page.trust.textContent, "Développement — non vérifié");
  assert.match(cardText(card(page, /Warframe Market/u)), /Langues en/u);
});

test("accepts sixteen exact localized listing entries", async () => {
  const localized = withTargets((targets) => {
    targets[0].listing.localizations = SIXTEEN_LOCALES.map((locale) => ({
      locale,
      name: locale === "en" ? "Warframe Market" : `Name ${locale}`,
      description: `Description ${locale}`,
    }));
    return targets;
  });
  const page = await run(localized);
  assert.ok(card(page, /Warframe Market/u));
});

test("sets preview src only for its exact immutable object URL", async () => {
  const sha256 = "c".repeat(64);
  const preview = withTargets((targets) => {
    const target = targets[0];
    target.preview = {
      url: `${DEVELOPMENT_BASE}previews/${target.manifest.id}/${target.manifest.version}/${sha256}.png`,
      mediaType: "image/png",
      size: 1024,
      sha256,
    };
    return targets;
  });
  const page = await run(preview);
  const image = descendants(card(page, /Warframe Market/u))
    .find((element) => element.tagName === "img");
  assert.ok(image);
  assert.equal(
    image.getAttribute("src"),
    `${DEVELOPMENT_BASE}previews/com.playervox.overcrow.warframe.market/2.0.0/${sha256}.png`,
  );
  assert.equal(image.getAttribute("alt"), "");
});

test("rejects a policy that differs from the fixed trust configuration", async () => {
  await assert.rejects(
    run(generated(), {
      policyObject: {
        mode: "development",
        keyId: "overcrow-development-2026",
        catalogUrl: "/marketplace/v1/catalog.json",
        objectBaseUrl: "https://example.test/marketplace/v1/",
        labels: {
          en: "Development — unverified",
          fr: "Développement — non vérifié",
        },
      },
    }),
    /marketplace policy/u,
  );
});

const invalidCatalogs = [
  ["lying length", "x".repeat(1024 * 1024 + 1), { contentLength: "1" }],
  ["missing length", "x".repeat(1024 * 1024 + 1), {}],
  ["absent stream", generated(), { noBody: true }],
  ["malformed streamed response", generated(), { malformedChunk: true }],
  ["malformed envelope", "{}", {}],
  ["wrong key ID", (() => {
    const parsed = JSON.parse(generated());
    parsed.keyId = "overcrow-production-2026-01";
    return JSON.stringify(parsed);
  })(), {}],
  ["invalid source URL", withTargets((targets) => {
    targets[0].listing.sourceUrl = "not a URL";
    return targets;
  }), {}],
  ["non-HTTPS source URL", withTargets((targets) => {
    targets[0].listing.sourceUrl = "http://example.test/source";
    return targets;
  }), {}],
  ["wrong object origin", withTargets((targets) => {
    targets[0].packageUrl = targets[0].packageUrl.replace(DEVELOPMENT_BASE, "https://example.test/");
    return targets;
  }), {}],
  ["path-bearing package version", withTargets((targets) => {
    targets[0].manifest.version = "../escape";
    targets[0].packageUrl = `${DEVELOPMENT_BASE}packages/${targets[0].manifest.id}/${targets[0].manifest.version}/${targets[0].packageSha256}.ocpkg`;
    return targets;
  }), {}],
  ["external preview URL", withTargets((targets) => {
    const sha256 = "d".repeat(64);
    targets[0].preview = {
      url: `https://example.test/${sha256}.png`,
      mediaType: "image/png",
      size: 1024,
      sha256,
    };
    return targets;
  }), {}],
  ["duplicate target IDs", withTargets((targets) => {
    targets.push(structuredClone(targets[0]));
    return targets;
  }), {}],
  ["duplicate listing locales", withTargets((targets) => {
    targets[0].listing.localizations[1].locale = "en";
    return targets;
  }), {}],
  ["listing without default locale", withTargets((targets) => {
    targets[0].listing.defaultLocale = "de";
    return targets;
  }), {}],
  ["native kind leftover", withTargets((targets) => {
    targets[0].manifest.kind = "provider";
    return targets;
  }), {}],
  ["more than 500 targets", withTargets((targets) => {
    const widget = targets[0];
    while (targets.length <= 500) {
      const copy = structuredClone(widget);
      copy.manifest.id = `com.playervox.overcrow.item${targets.length}`;
      copy.packageUrl = `${DEVELOPMENT_BASE}packages/${copy.manifest.id}/${copy.manifest.version}/${copy.packageSha256}.ocpkg`;
      targets.push(copy);
    }
    return targets;
  }), {}],
];

for (const [name, body, options] of invalidCatalogs) {
  test(`rejects ${name}`, async () => unavailable(await run(body, options)));
}
