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
    ...["page-title", "page-description", "language-label", "catalog-heading", "skip-link"].map((id) => [id, new Element()]),
  ]);
  const document = {
    createElement: (tag) => new Element(tag),
    getElementById: (id) => elements.get(id),
    documentElement: { lang: "en" },
  };
  const requests = [];
  const navigation = [];
  const timers = [];
  const context = {
    document,
    TextDecoder,
    Uint8Array,
    URL,
    open: (...args) => navigation.push(args),
    setTimeout: (...args) => timers.push(args),
    setInterval: (...args) => timers.push(args),
    atob: (value) => Buffer.from(value, "base64").toString("binary"),
    fetch: async (url) => {
      requests.push(url);
      if (options.responseGate) await options.responseGate;
      return {
        ok: options.ok ?? true,
        headers: { get: () => options.contentLength },
        body: options.noBody ? undefined : streamed(body, options),
      };
    },
  };
  const location = {
    set href(value) { navigation.push(value); },
    assign: (value) => navigation.push(value),
    replace: (value) => navigation.push(value),
  };
  Object.defineProperty(context, "location", {
    get: () => location,
    set: (value) => navigation.push(value),
  });
  context.window = context;
  if (options.policyObject) {
    context.overcrowMarketplacePolicy = options.policyObject;
  } else {
    const policyPath = options.policy || "web/marketplace/policies/development.js";
    vm.runInNewContext(fs.readFileSync(policyPath, "utf8"), context);
  }
  vm.runInNewContext(fs.readFileSync("web/marketplace/app.js", "utf8"), context);
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  return { catalog, language, trust, document, requests, navigation, timers, elements };
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

test("localizes the complete page frame and pending catalog state", async () => {
  let resolve;
  const responseGate = new Promise((done) => { resolve = done; });
  const page = await run(generated(), { responseGate });
  assert.equal(page.elements.get("page-title").textContent, "Widget marketplace");
  assert.equal(page.elements.get("language-label").textContent, "Language");
  assert.equal(page.catalog.getAttribute("aria-busy"), "true");
  assert.match(cardText(page.catalog), /Loading widgets/u);
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.equal(page.elements.get("page-title").textContent, "Catalogue de widgets");
  assert.equal(page.document.title, "Catalogue de widgets · OverCrow");
  assert.equal(page.elements.get("language-label").textContent, "Langue");
  assert.equal(page.elements.get("catalog-heading").textContent, "Widgets disponibles");
  assert.equal(page.elements.get("skip-link").textContent, "Aller aux widgets");
  assert.match(page.elements.get("page-description").textContent, /Centre de contrôle OverCrow/u);
  assert.match(cardText(page.catalog), /Chargement des widgets/u);
  assert.equal(page.trust.textContent, "Développement — non vérifié");
  resolve();
  await new Promise((done) => setImmediate(done));
  assert.equal(page.catalog.getAttribute("aria-busy"), "false");
  assert.ok(card(page, /Marché Warframe/u));
});

test("distinguishes an empty valid catalog from errors in both languages", async () => {
  const page = await run(envelope([]));
  assert.equal(page.catalog.getAttribute("aria-busy"), "false");
  assert.equal(cardText(page.catalog), "No widgets are listed yet.");
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.equal(cardText(page.catalog), "Aucun widget n’est encore proposé.");
});

test("uses a readable source label while keeping the exact source URL accessible", async () => {
  const page = await run(generated());
  for (const [locale, label] of [["en", "View source"], ["fr", "Voir le code source"]]) {
    page.language.value = locale;
    page.language.dispatch("change");
    const source = descendants(page.catalog).find((element) => element.textContent === label);
    assert.ok(source);
    assert.equal(source.getAttribute("href"), webTarget().listing.sourceUrl);
    assert.equal(source.getAttribute("title"), webTarget().listing.sourceUrl);
    assert.equal(source.getAttribute("rel"), "noreferrer noopener");
  }
});

test("describes an empty permission set without inventing grants", async () => {
  const page = await run(envelope([webTarget({ manifest: { permissions: {} } })]));
  assert.match(cardText(page.catalog), /No permissions requested\./u);
  assert.doesNotMatch(cardText(page.catalog), /Fetches public data|Receives OverCrow game events|Stores private widget data|Writes to clipboard/u);
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.match(cardText(page.catalog), /Aucune permission demandée\./u);
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

test("each validated card links explicitly to OverCrow without navigation or downloads", async () => {
  const targets = [webTarget(), webTarget({ id: "org.example.other-widget" })];
  const page = await run(envelope(targets));
  for (const [locale, label, explanation] of [
    ["en", "Open in OverCrow", /Requires the OverCrow app/u],
    ["fr", "Ouvrir dans OverCrow", /Nécessite l’application OverCrow/u],
  ]) {
    page.language.value = locale;
    page.language.dispatch("change");
    assert.equal(page.catalog.children.length, targets.length);
    for (const [index, item] of page.catalog.children.entries()) {
      const link = descendants(item).find((element) => element.textContent === label);
      assert.ok(link);
      assert.equal(link.tagName, "a");
      assert.equal(link.getAttribute("href"), `overcrow://widget/${targets[index].manifest.id}`);
      assert.equal(link.listeners.size, 0);
      assert.match(cardText(item), explanation);
    }
  }
  assert.deepEqual(page.navigation, []);
  assert.deepEqual(page.timers, []);
  assert.deepEqual(page.requests, ["/marketplace/v1/catalog.json"]);
});

test("selects one latest version per ID regardless of catalog order", async () => {
  const versions = ["2.0.9", "2.0.10", "1.99.99"];
  for (const order of [versions, [...versions].reverse(), [versions[2], versions[0], versions[1]]]) {
    const page = await run(envelope([
      ...order.map((version) => webTarget({ version })),
      webTarget({ id: "org.example.other-widget", version: "0.1.0" }),
    ]));
    assert.equal(page.catalog.children.length, 2);
    assert.match(cardText(page.catalog.children[0]), /Version 2\.0\.10\n/u);
    assert.match(cardText(page.catalog.children[1]), /Version 0\.1\.0\n/u);
  }
});

test("uses Rust SemVer ordering for prereleases, large numbers, and build metadata", async () => {
  const orderedGroups = [
    ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.2", "1.0.0-alpha.10", "1.0.0-beta", "1.0.0-rc.1", "1.0.0"],
    ["1.0.0-999999999999999999999999", "1.0.0-A", "1.0.0-a"],
    ["9007199254740992.0.0", "9007199254740993.0.0", "18446744073709551615.0.0"],
    ["1.0.0", "1.0.0+0", "1.0.0+00", "1.0.0+2", "1.0.0+02", "1.0.0+10", "1.0.0+A", "1.0.0+a", "1.0.0+a.1"],
    ["1.0.0-rc.1+z", "1.0.0+0"],
  ];
  for (const versions of orderedGroups) {
    for (let index = 1; index < versions.length; index += 1) {
      for (const order of [[versions[index - 1], versions[index]], [versions[index], versions[index - 1]]]) {
        const page = await run(envelope(order.map((version) => webTarget({ version }))));
        assert.equal(page.catalog.children.length, 1);
        assert.ok(cardText(page.catalog.children[0]).includes(`Version ${versions[index]}\n`), order.join(" < "));
      }
    }
  }
});

test("latest revoked or suspended versions never expose an older verified card", async () => {
  for (const [status, label] of [
    ["revoked", "Revoked catalog entry"],
    ["security-suspended", "Security-suspended catalog entry"],
  ]) {
    const targets = [webTarget({ version: "1.0.0" }), webTarget({ version: "2.0.0", target: { status } })];
    for (const order of [targets, [...targets].reverse()]) {
      const page = await run(envelope(order));
      assert.equal(page.catalog.children.length, 1);
      assert.match(cardText(page.catalog.children[0]), /Version 2\.0\.0\n/u);
      assert.ok(cardText(page.catalog.children[0]).includes(label));
      assert.doesNotMatch(cardText(page.catalog.children[0]), /Verified catalog entry/u);
    }
  }
});

test("an older signed revocation coexists with a newer verified version", async () => {
  const targets = [
    webTarget({ version: "1.0.0", target: { status: "revoked" } }),
    webTarget({ version: "2.0.0" }),
  ];
  for (const order of [targets, [...targets].reverse()]) {
    const page = await run(envelope(order));
    assert.equal(page.catalog.children.length, 1);
    assert.match(cardText(page.catalog.children[0]), /Version 2\.0\.0\n/u);
    assert.match(cardText(page.catalog.children[0]), /Verified catalog entry/u);
    assert.doesNotMatch(cardText(page.catalog.children[0]), /Revoked catalog entry/u);
  }
});

test("invalid IDs cannot become application URLs", async () => {
  for (const id of [
    "org.example/widget", "org.example?install=true", "org.example#activate",
    "org.example%2fwidget", "org.Example.widget", "overcrow://widget/org.example",
  ]) {
    const page = await run(envelope([webTarget({ id })]));
    unavailable(page);
    assert.equal(descendants(page.catalog).some((element) => element.tagName === "a"), false);
    assert.deepEqual(page.navigation, []);
    assert.deepEqual(page.timers, []);
  }
});

test("rejects noncanonical or out-of-range Rust SemVer versions", async () => {
  for (const version of [
    "01.0.0", "1.0.0-01", "1.0.0-alpha.01", "18446744073709551616.0.0",
    "0.18446744073709551616.0", "0.0.18446744073709551616", "1.0.0+", "1.0.0-a..b",
  ]) unavailable(await run(envelope([webTarget({ version })])));
});

test("rejects duplicate ID/version even when package metadata or status differs", async () => {
  unavailable(await run(envelope([
    webTarget({ version: "1.0.0+02" }),
    webTarget({ version: "1.0.0+02", packageSha256: "c".repeat(64), target: { status: "revoked" } }),
  ])));
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

test("renders admitted permissions beyond sixteen entries", async () => {
  const body = withTargets((targets) => {
    targets[0].manifest.permissions.network = Array.from({ length: 17 }, (_, index) => ({
      origin: `https://api${index}.example.test`,
      method: "GET",
      pathPrefix: "/v2/",
    }));
    targets[0].manifest.permissions.gameEvents = Array.from(
      { length: 17 }, (_, index) => `overcrow.game.event${index}.v1`,
    );
    return targets;
  });
  const page = await run(body);
  const market = card(page, /Warframe Market/u);
  assert.ok(market);
  assert.match(cardText(market), /api16\.example\.test/u);
  assert.match(cardText(market), /Receives OverCrow game events/u);
});

test("keeps catalog failures visible when the language changes", async () => {
  const page = await run(generated(), { ok: false });
  unavailable(page);
  page.language.value = "fr";
  page.language.dispatch("change");
  assert.equal(page.catalog.children.length, 1);
  assert.equal(page.catalog.children[0].textContent, "Catalogue indisponible.");
  page.language.value = "en";
  page.language.dispatch("change");
  unavailable(page);
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
  ["duplicate target ID/version", withTargets((targets) => {
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
