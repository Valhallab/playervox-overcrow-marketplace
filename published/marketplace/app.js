"use strict";

const MAX_ENVELOPE_BYTES = 1024 * 1024;
const MAX_STREAM_CHUNKS = 4096;
const MAX_TARGETS = 500;
const policy = globalThis.overcrowMarketplacePolicy;
const fixedPolicies = {
  development: {
    keyId: "overcrow-development-2026",
    catalogUrl: "/marketplace/v1/catalog.json",
    objectBaseUrl: "http://127.0.0.1:8787/marketplace/v1/",
    labels: {
      en: "Development — unverified",
      fr: "Développement — non vérifié",
    },
  },
  production: {
    keyId: "overcrow-production-2026-01",
    catalogUrl: "/marketplace/v1/catalog.json",
    objectBaseUrl: "https://overcrow.playervox.com/marketplace/v1/",
    labels: {
      en: "Production catalog — installs are verified by OverCrow",
      fr: "Catalogue de production — les installations sont vérifiées par OverCrow",
    },
  },
};

function exactKeys(value, keys) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  return actual.length === keys.length
    && actual.every((key, index) => key === [...keys].sort()[index]);
}

function validPolicy(value) {
  if (!exactKeys(value, ["mode", "keyId", "catalogUrl", "objectBaseUrl", "labels"])) {
    return false;
  }
  const expected = fixedPolicies[value.mode];
  return expected
    && exactKeys(value.labels, ["en", "fr"])
    && value.keyId === expected.keyId
    && value.catalogUrl === expected.catalogUrl
    && value.objectBaseUrl === expected.objectBaseUrl
    && value.labels.en === expected.labels.en
    && value.labels.fr === expected.labels.fr;
}

if (!validPolicy(policy)) throw new Error("marketplace policy");

const state = { locale: "en", targets: [] };
const catalog = document.getElementById("catalog");
const language = document.getElementById("language");
const trust = document.getElementById("trust-label");
const copy = {
  en: {
    version: "Version",
    author: "Author",
    source: "Source",
    license: "License",
    languages: "Languages",
    gameScope: "Game scope",
    steamApp: "Steam app",
    steamApps: "Steam apps",
    allGames: "All games",
    http: "Fetches public data from",
    session: "Reads OverCrow session data",
    storage: "Stores private widget settings",
    clipboard: "Writes a trade message to clipboard",
    provider: "Publishes bounded public world-state data",
    dependency: "Includes dependency",
    dependencies: "Includes dependencies",
    standalone: "Standalone package",
    verified: "Verified catalog entry",
    suspended: "Security-suspended catalog entry",
    revoked: "Revoked catalog entry",
  },
  fr: {
    version: "Version",
    author: "Auteur",
    source: "Source",
    license: "Licence",
    languages: "Langues",
    gameScope: "Périmètre de jeu",
    steamApp: "Application Steam",
    steamApps: "Applications Steam",
    allGames: "Tous les jeux",
    http: "Récupère des données publiques depuis",
    session: "Lit les données de session OverCrow",
    storage: "Stocke des réglages privés du widget",
    clipboard: "Écrit un message d’échange dans le presse-papiers",
    provider: "Publie des données publiques bornées de l’état mondial",
    dependency: "Inclut la dépendance",
    dependencies: "Inclut les dépendances",
    standalone: "Paquet autonome",
    verified: "Entrée de catalogue vérifiée",
    suspended: "Entrée de catalogue suspendue pour sécurité",
    revoked: "Entrée de catalogue révoquée",
  },
};

function string(value, maximum) {
  return typeof value === "string" && value.length > 0 && value.length <= maximum;
}

function digest(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function locale(value) {
  return typeof value === "string" && /^[a-z]{2}(?:-[A-Z]{2})?$/u.test(value);
}

function extensionId(value) {
  return string(value, 128)
    && value.split(".").length >= 2
    && value.split(".").every((segment) => (
      /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u.test(segment)
    ));
}

function version(value) {
  return string(value, 64)
    && /^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/u.test(value);
}

function httpsUrl(value) {
  if (!string(value, 512)) return false;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:"
      && !parsed.username
      && !parsed.password;
  } catch {
    return false;
  }
}

function httpHost(value) {
  if (!string(value, 253) || !value.includes(".")) return false;
  return value.split(".").every((label) => (
    /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u.test(label)
  ));
}

function immutableObjectUrl(area, manifest, sha256, extension) {
  return `${policy.objectBaseUrl}${area}/${manifest.id}/${manifest.version}/${sha256}.${extension}`;
}

function decode(value) {
  if (!string(value, Math.ceil(MAX_ENVELOPE_BYTES * 4 / 3))) {
    throw new Error("payload");
  }
  const bytes = Uint8Array.from(
    atob(value.replace(/-/gu, "+").replace(/_/gu, "/").padEnd(Math.ceil(value.length / 4) * 4, "=")),
    (item) => item.charCodeAt(0),
  );
  if (bytes.length > MAX_ENVELOPE_BYTES) throw new Error("payload");
  return JSON.parse(new TextDecoder().decode(bytes));
}

function capabilities(value) {
  if (!exactKeys(value, ["http", "gameData", "storage", "clipboardWrite", "provider"])) {
    return false;
  }
  if (!Array.isArray(value.http) || value.http.length > 16) return false;
  if (new Set(value.http).size !== value.http.length || !value.http.every(httpHost)) return false;
  if (!Array.isArray(value.gameData) || value.gameData.length > 1) return false;
  if (new Set(value.gameData).size !== value.gameData.length
      || !value.gameData.every((feed) => feed === "overcrow.session.v1")) return false;
  return typeof value.storage === "boolean"
    && typeof value.clipboardWrite === "boolean"
    && typeof value.provider === "boolean";
}

function locales(manifest, listing) {
  const available = manifest.availableLocales;
  const localized = listing.localizations;
  if (!Array.isArray(available) || available.length === 0 || available.length > 32) return false;
  if (!available.every(locale) || new Set(available).size !== available.length) return false;
  if (!locale(manifest.defaultLocale)
      || !available.includes(manifest.defaultLocale)
      || !available.includes("en")) return false;
  if (!Array.isArray(localized) || localized.length !== available.length) return false;
  const listed = new Set();
  for (const entry of localized) {
    if (!entry || !locale(entry.locale) || listed.has(entry.locale)
        || !string(entry.name, 128) || !string(entry.description, 512)) return false;
    listed.add(entry.locale);
  }
  return available.every((entry) => listed.has(entry));
}

function games(value) {
  if (!Array.isArray(value) || value.length > 32) return false;
  const ids = new Set();
  return value.every((game) => {
    if (!game || game.platform !== "steam" || typeof game.id !== "string") return false;
    const id = Number(game.id);
    return Number.isInteger(id) && id > 0 && id <= 4294967295
      && String(id) === game.id && !ids.has(game.id) && Boolean(ids.add(game.id));
  });
}

function dependencies(value) {
  if (!Array.isArray(value) || value.length > 32) return false;
  const ids = new Set();
  return value.every((dependency) => dependency
    && extensionId(dependency.id)
    && version(dependency.version)
    && digest(dependency.sha256)
    && !ids.has(dependency.id)
    && Boolean(ids.add(dependency.id)));
}

function preview(value, manifest) {
  if (value === undefined) return true;
  return manifest.kind !== "provider"
    && value
    && value.mediaType === "image/png"
    && Number.isSafeInteger(value.size)
    && value.size > 0
    && value.size <= 256 * 1024
    && digest(value.sha256)
    && value.url === immutableObjectUrl("previews", manifest, value.sha256, "png");
}

function target(value) {
  const manifest = value && value.manifest;
  const listing = value && value.listing;
  if (!value || !manifest || !listing) return false;
  if (!["verified", "security-suspended", "revoked"].includes(value.status)) return false;
  if (manifest.schemaVersion !== 1 || !extensionId(manifest.id)
      || !version(manifest.version)
      || !["widget", "bundle", "provider"].includes(manifest.kind)
      || manifest.apiVersion !== "1") return false;
  if (!locales(manifest, listing) || !games(manifest.games)
      || !dependencies(manifest.dependencies) || !capabilities(manifest.capabilities)) return false;
  if (!string(listing.author, 128)
      || !string(listing.spdxLicense, 64)
      || !/^[A-Za-z0-9][A-Za-z0-9.+-]{0,63}$/u.test(listing.spdxLicense)
      || !httpsUrl(listing.sourceUrl)) return false;
  return Number.isSafeInteger(value.packageSize)
    && value.packageSize > 0
    && value.packageSize <= 16 * 1024 * 1024
    && digest(value.packageSha256)
    && value.minHostApi === 1
    && value.maxHostApi === 1
    && value.packageUrl === immutableObjectUrl("packages", manifest, value.packageSha256, "ocpkg")
    && preview(value.preview, manifest);
}

function timestamp(value) {
  return string(value, 40) && Number.isFinite(Date.parse(value));
}

function validateDependencies(targets, byId) {
  const edges = new Map();
  for (const item of targets) {
    const dependenciesForTarget = [];
    for (const dependency of item.manifest.dependencies) {
      const provider = byId.get(dependency.id);
      if (!provider
          || provider.manifest.kind !== "provider"
          || provider.status !== "verified"
          || provider.manifest.version !== dependency.version
          || provider.packageSha256 !== dependency.sha256) throw new Error("dependency");
      dependenciesForTarget.push(provider);
    }
    edges.set(item.manifest.id, dependenciesForTarget);
  }
  const active = new Set();
  const complete = new Set();
  function visit(item) {
    if (active.has(item.manifest.id)) throw new Error("dependency");
    if (complete.has(item.manifest.id)) return;
    active.add(item.manifest.id);
    for (const dependency of edges.get(item.manifest.id)) visit(dependency);
    active.delete(item.manifest.id);
    complete.add(item.manifest.id);
  }
  for (const item of targets) visit(item);
}

function validate(text) {
  if (typeof text !== "string" || text.length === 0 || text.length > MAX_ENVELOPE_BYTES) {
    throw new Error("envelope");
  }
  const envelope = JSON.parse(text);
  if (!envelope || envelope.schemaVersion !== 1 || envelope.keyId !== policy.keyId
      || !string(envelope.payload, Math.ceil(MAX_ENVELOPE_BYTES * 4 / 3))
      || !string(envelope.signature, 128)) throw new Error("envelope");
  const payload = decode(envelope.payload);
  if (!payload || payload.schemaVersion !== 1
      || !Number.isSafeInteger(payload.sequence) || payload.sequence < 1
      || !timestamp(payload.generatedAt) || !timestamp(payload.expiresAt)
      || Date.parse(payload.generatedAt) >= Date.parse(payload.expiresAt)
      || !Array.isArray(payload.targets) || payload.targets.length === 0
      || payload.targets.length > MAX_TARGETS || !payload.targets.every(target)) {
    throw new Error("payload");
  }
  const byId = new Map();
  for (const item of payload.targets) {
    if (byId.has(item.manifest.id)) throw new Error("target");
    byId.set(item.manifest.id, item);
  }
  validateDependencies(payload.targets, byId);
  return payload.targets;
}

function localized(item) {
  return item.listing.localizations.find((text) => text.locale === state.locale)
    || item.listing.localizations.find((text) => text.locale === "en");
}

function dependencyClosure(item) {
  const entries = [item];
  const included = new Set([item.manifest.id]);
  for (let index = 0; index < entries.length; index += 1) {
    for (const dependency of entries[index].manifest.dependencies) {
      if (!included.has(dependency.id)) {
        included.add(dependency.id);
        entries.push(state.targets.find((candidate) => candidate.manifest.id === dependency.id));
      }
    }
  }
  return entries;
}

function details(item) {
  const languageCopy = copy[state.locale];
  const entries = dependencyClosure(item);
  const capabilitySet = entries.map((entry) => entry.manifest.capabilities);
  const hosts = [...new Set(capabilitySet.flatMap((value) => value.http))];
  const values = [];
  const dependencyNames = entries.slice(1).map((dependency) => localized(dependency).name);
  if (dependencyNames.length === 0) {
    values.push(languageCopy.standalone);
  } else {
    const label = dependencyNames.length === 1
      ? languageCopy.dependency
      : languageCopy.dependencies;
    values.push(`${label}: ${dependencyNames.join(", ")}`);
  }
  if (hosts.length) values.push(`${languageCopy.http}: ${hosts.join(", ")}`);
  if (capabilitySet.some((value) => value.gameData.includes("overcrow.session.v1"))) {
    values.push(languageCopy.session);
  }
  if (capabilitySet.some((value) => value.storage)) values.push(languageCopy.storage);
  if (capabilitySet.some((value) => value.clipboardWrite)) values.push(languageCopy.clipboard);
  if (capabilitySet.some((value) => value.provider)) values.push(languageCopy.provider);
  return values;
}

function textElement(tag, value) {
  const element = document.createElement(tag);
  element.textContent = value;
  return element;
}

function gameScope(item, languageCopy) {
  const ids = item.manifest.games.map((game) => game.id);
  if (ids.length === 0) return `${languageCopy.gameScope} ${languageCopy.allGames}`;
  const label = ids.length === 1 ? languageCopy.steamApp : languageCopy.steamApps;
  return `${label} ${ids.join(", ")}`;
}

function statusLabel(status, languageCopy) {
  if (status === "verified") return languageCopy.verified;
  if (status === "security-suspended") return languageCopy.suspended;
  return languageCopy.revoked;
}

function card(item) {
  const text = localized(item);
  const languageCopy = copy[state.locale];
  const element = document.createElement("article");
  element.setAttribute("class", "card");
  if (item.preview) {
    const image = document.createElement("img");
    image.setAttribute("class", "preview");
    image.setAttribute("src", item.preview.url);
    image.setAttribute("alt", "");
    element.append(image);
  }
  element.append(
    textElement("h2", text.name),
    textElement("p", text.description),
    textElement("p", `${languageCopy.version} ${item.manifest.version}`),
    textElement("p", `${languageCopy.author} ${item.listing.author}`),
    textElement("p", `${languageCopy.license} ${item.listing.spdxLicense}`),
    textElement("p", `${languageCopy.languages} ${item.manifest.availableLocales.join(", ")}`),
    textElement("p", gameScope(item, languageCopy)),
  );
  const source = document.createElement("a");
  source.textContent = `${languageCopy.source} ${item.listing.sourceUrl}`;
  source.setAttribute("href", item.listing.sourceUrl);
  source.setAttribute("rel", "noreferrer noopener");
  element.append(source);
  for (const value of details(item)) element.append(textElement("p", value));
  element.append(textElement("p", statusLabel(item.status, languageCopy)));
  return element;
}

function render() {
  catalog.textContent = "";
  trust.textContent = policy.labels[state.locale];
  for (const item of state.targets) {
    if (item.manifest.kind !== "provider") catalog.append(card(item));
  }
}

function unavailable() {
  const message = document.createElement("p");
  message.textContent = "Catalog unavailable.";
  catalog.textContent = "";
  catalog.append(message);
}

async function readBounded(response) {
  if (!response.body || typeof response.body.getReader !== "function") throw new Error("stream");
  const reader = response.body.getReader();
  if (!reader || typeof reader.read !== "function") throw new Error("stream");
  const bytes = new Uint8Array(MAX_ENVELOPE_BYTES);
  let chunkCount = 0;
  let total = 0;
  for (;;) {
    const part = await reader.read();
    if (!part || typeof part !== "object" || typeof part.done !== "boolean") {
      throw new Error("stream");
    }
    if (part.done) {
      if (part.value !== undefined) throw new Error("stream");
      break;
    }
    chunkCount += 1;
    if (chunkCount > MAX_STREAM_CHUNKS
        || !(part.value instanceof Uint8Array)
        || part.value.length === 0
        || part.value.length > MAX_ENVELOPE_BYTES - total) throw new Error("stream");
    bytes.set(part.value, total);
    total += part.value.length;
  }
  return new TextDecoder().decode(bytes.subarray(0, total));
}

language.addEventListener("change", () => {
  state.locale = language.value === "fr" ? "fr" : "en";
  document.documentElement.lang = state.locale;
  render();
});

fetch(policy.catalogUrl)
  .then((response) => {
    if (!response.ok) throw new Error("catalog unavailable");
    return readBounded(response);
  })
  .then((text) => {
    state.targets = validate(text);
    render();
  })
  .catch(unavailable);
