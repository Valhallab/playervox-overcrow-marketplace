"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const vm = require("node:vm");

const DEVELOPMENT_BASE = "http://127.0.0.1:8787/marketplace/v1/";
const PRODUCTION_BASE = "https://overcrow.playervox.com/marketplace/v1/";

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

const catalogPath = process.argv[2] || "tests/fixtures/development-catalog.json";

function generated() {
  return fs.readFileSync(catalogPath, "utf8");
}

function payload(body) {
  const envelope = JSON.parse(body);
  return JSON.parse(Buffer.from(envelope.payload, "base64url"));
}

function withPayload(change, source = generated()) {
  const envelope = JSON.parse(source);
  const body = JSON.parse(Buffer.from(envelope.payload, "base64url"));
  envelope.payload = Buffer.from(JSON.stringify(change(body))).toString("base64url");
  return JSON.stringify(envelope);
}

function withTargets(change, source = generated()) {
  return withPayload((body) => {
    const targets = change(body.targets);
    assert.equal(
      targets.filter((target) => target.manifest.kind === "provider").length,
      1,
    );
    return { ...body, targets };
  }, source);
}

function withListingName(name) {
  return withTargets((targets) => {
    const widget = targets.find((target) => target.manifest.kind === "widget");
    widget.listing.localizations[0].name = name;
    return targets;
  });
}

function productionCatalog() {
  const envelope = JSON.parse(generated());
  envelope.keyId = "overcrow-production-2026-01";
  const body = JSON.parse(Buffer.from(envelope.payload, "base64url"));
  for (const target of body.targets) {
    target.packageUrl = target.packageUrl.replace(DEVELOPMENT_BASE, PRODUCTION_BASE);
    if (target.preview) {
      target.preview.url = target.preview.url.replace(DEVELOPMENT_BASE, PRODUCTION_BASE);
    }
  }
  envelope.payload = Buffer.from(JSON.stringify(body)).toString("base64url");
  return JSON.stringify(envelope);
}

function streamed(body, options) {
  const chunks = options.malformedChunk
    ? ["not bytes"]
    : [new Uint8Array(Buffer.from(body))];
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

test("development mode renders the catalog and hides providers", async () => {
  const body = generated();
  const actual = payload(body);
  assert.equal(actual.targets.length, 6);
  const page = await run(body);
  const index = fs.readFileSync("web/marketplace/index.html", "utf8");
  assert.match(index, /<option value="en" selected>English<\/option>/u);
  assert.match(index, /<option value="fr">Français<\/option>/u);
  assert.deepEqual(page.requests, ["/marketplace/v1/catalog.json"]);
  assert.equal(page.trust.textContent, "Development — unverified");
  assert.equal(page.catalog.children.length, 5);
  const fissures = cardText(card(page, /Warframe Void Fissures/u));
  for (const expected of [
    /Includes dependency: Warframe Worldstate Provider/u,
    /Fetches public data from: api\.warframe\.com/u,
    /Reads OverCrow session data/u,
    /Stores private widget settings/u,
    /Publishes bounded public world-state data/u,
  ]) assert.match(fissures, expected);
  const market = cardText(card(page, /Warframe Market/u));
  for (const expected of [
    /api\.warframe\.market/u,
    /Reads OverCrow session data/u,
    /Stores private widget settings/u,
    /Writes a trade message to clipboard/u,
  ]) assert.match(market, expected);
  assert.doesNotMatch(
    page.catalog.children.map(cardText).join("\n"),
    /Shares bounded public Warframe world-state data/u,
  );
});

test("production mode renders complete catalog metadata without a development claim", async () => {
  const page = await run(productionCatalog(), {
    policy: "web/marketplace/policies/production.js",
  });
  assert.equal(
    page.trust.textContent,
    "Production catalog — installs are verified by OverCrow",
  );
  const fissures = cardText(card(page, /Warframe Void Fissures/u));
  for (const expected of [
    "Version 1.0.0",
    "Author PlayerVox",
    "License AGPL-3.0-only",
    "Languages en, fr",
    "Steam app 230410",
    "Warframe Worldstate Provider",
    "api.warframe.com",
    "Verified catalog entry",
  ]) assert.match(fissures, new RegExp(expected, "u"));
  assert.doesNotMatch(fissures, /Development — unverified/u);

  const source = descendants(card(page, /Warframe Void Fissures/u))
    .find((element) => element.tagName === "a");
  assert.equal(
    source.getAttribute("href"),
    "https://github.com/PlayerVox/playervox-overcrow-marketplace",
  );
  assert.equal(source.getAttribute("rel"), "noreferrer noopener");
});

test("creator strings are assigned only through textContent", async () => {
  const poisoned = withListingName("<img src=x onerror=globalThis.pwned=true>");
  const page = await run(poisoned);
  const item = card(page, /<img src=x/u);
  assert.ok(item);
  assert.equal(item.children.some((child) => child.tagName === "script"), false);
  assert.equal(descendants(item).some((child) => child.tagName === "img"), false);
  assert.match(cardText(item), /<img src=x onerror=globalThis\.pwned=true>/u);
});

test("French UI falls back to English creator copy when only English is supplied", async () => {
  const englishOnly = withTargets((targets) => {
    const widget = targets.find((target) => target.manifest.kind === "widget");
    widget.manifest.availableLocales = ["en"];
    widget.listing.localizations = widget.listing.localizations.filter(
      (entry) => entry.locale === "en",
    );
    return targets;
  });
  const page = await run(englishOnly);
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.equal(page.document.documentElement.lang, "fr");
  assert.equal(page.trust.textContent, "Développement — non vérifié");
  assert.match(cardText(card(page, /Warframe Void Fissures/u)), /Langues en/u);
});

test("renders generic bounded Steam scopes", async () => {
  const multipleGames = withTargets((targets) => {
    const widget = targets.find((target) => target.manifest.kind === "widget");
    widget.manifest.games = [
      { platform: "steam", id: "230410" },
      { platform: "steam", id: "440" },
    ];
    return targets;
  });
  const page = await run(multipleGames);
  assert.match(
    cardText(card(page, /Warframe Void Fissures/u)),
    /Steam apps 230410, 440/u,
  );
});

test("renders an empty Steam scope as all games", async () => {
  const allGames = withTargets((targets) => {
    const widget = targets.find((target) => target.manifest.kind === "widget");
    widget.manifest.games = [];
    return targets;
  });
  const page = await run(allGames);
  assert.match(
    cardText(card(page, /Warframe Void Fissures/u)),
    /Game scope All games/u,
  );
});

test("sets preview src only for its exact immutable object URL", async () => {
  const sha256 = "a".repeat(64);
  const preview = withTargets((targets) => {
    const widget = targets.find((target) => target.manifest.kind === "widget");
    widget.preview = {
      url: `${DEVELOPMENT_BASE}previews/${widget.manifest.id}/${widget.manifest.version}/${sha256}.png`,
      mediaType: "image/png",
      size: 1024,
      sha256,
    };
    return targets;
  });
  const page = await run(preview);
  const image = descendants(card(page, /Warframe Void Fissures/u))
    .find((element) => element.tagName === "img");
  assert.ok(image);
  assert.equal(
    image.getAttribute("src"),
    `${DEVELOPMENT_BASE}previews/com.playervox.overcrow.warframe.fissures/1.0.0/${sha256}.png`,
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

const firstConsumer = (targets) => targets.findIndex(
  (target) => target.manifest.kind === "widget",
);

const invalidCatalogs = [
  ["lying length", "x".repeat(1024 * 1024 + 1), { contentLength: "1" }],
  ["missing length", "x".repeat(1024 * 1024 + 1), {}],
  ["absent stream", generated(), { noBody: true }],
  ["malformed streamed response", generated(), { malformedChunk: true }],
  ["malformed envelope", "{}", {}],
  ["wrong key ID", (() => {
    const envelope = JSON.parse(generated());
    envelope.keyId = "overcrow-production-2026-01";
    return JSON.stringify(envelope);
  })(), {}],
  ["invalid source URL", withTargets((targets) => {
    targets[firstConsumer(targets)].listing.sourceUrl = "not a URL";
    return targets;
  }), {}],
  ["non-HTTPS source URL", withTargets((targets) => {
    targets[firstConsumer(targets)].listing.sourceUrl = "http://example.test/source";
    return targets;
  }), {}],
  ["wrong object origin", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.packageUrl = target.packageUrl.replace(DEVELOPMENT_BASE, "https://example.test/");
    return targets;
  }), {}],
  ["path-bearing package version", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.manifest.version = "../escape";
    target.packageUrl = `${DEVELOPMENT_BASE}packages/${target.manifest.id}/${target.manifest.version}/${target.packageSha256}.ocpkg`;
    return targets;
  }), {}],
  ["external preview URL", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    const sha256 = "a".repeat(64);
    target.preview = {
      url: `https://example.test/${sha256}.png`,
      mediaType: "image/png",
      size: 1024,
      sha256,
    };
    return targets;
  }), {}],
  ["duplicate target IDs", withTargets((targets) => {
    const widgets = targets.filter((target) => target.manifest.kind === "widget");
    widgets[1].manifest.id = widgets[0].manifest.id;
    widgets[1].packageUrl = `${DEVELOPMENT_BASE}packages/${widgets[1].manifest.id}/${widgets[1].manifest.version}/${widgets[1].packageSha256}.ocpkg`;
    return targets;
  }), {}],
  ["duplicate dependencies", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.manifest.dependencies.push({ ...target.manifest.dependencies[0] });
    return targets;
  }), {}],
  ["duplicate listing locales", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.listing.localizations[1].locale = "en";
    return targets;
  }), {}],
  ["duplicate available locales", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.manifest.availableLocales[1] = "en";
    return targets;
  }), {}],
  ["more than 32 Steam scopes", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.manifest.games = Array.from({ length: 33 }, (_, index) => ({
      platform: "steam",
      id: String(index + 1),
    }));
    return targets;
  }), {}],
  ["more than 16 HTTP capabilities", withTargets((targets) => {
    const target = targets[firstConsumer(targets)];
    target.manifest.capabilities.http = Array.from(
      { length: 17 },
      (_, index) => `api${index}.example.test`,
    );
    return targets;
  }), {}],
  ["more than 500 targets", withTargets((targets) => {
    const widget = targets[firstConsumer(targets)];
    while (targets.length <= 500) targets.push(structuredClone(widget));
    return targets;
  }), {}],
  ["wrong dependency version", withTargets((targets) => {
    targets[firstConsumer(targets)].manifest.dependencies[0].version = "9.9.9";
    return targets;
  }), {}],
  ["wrong dependency hash", withTargets((targets) => {
    targets[firstConsumer(targets)].manifest.dependencies[0].sha256 = "b".repeat(64);
    return targets;
  }), {}],
];

for (const [name, body, options] of invalidCatalogs) {
  test(`rejects ${name}`, async () => unavailable(await run(body, options)));
}
