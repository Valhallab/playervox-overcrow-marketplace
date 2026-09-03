"use strict";

const MAX_ENVELOPE_BYTES = 1024 * 1024;
const MAX_STREAM_CHUNKS = 4096;
const MAX_TARGETS = 500;
const MAX_PACKAGE_BYTES = 128 * 1024 * 1024;
const MAX_PREVIEW_BYTES = 256 * 1024;
const MAX_LISTING_LOCALES = 16;
const MAX_NETWORK_GRANTS = 16;
const MAX_GAME_EVENTS = 16;
const MAX_FILES = 4096;
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
    http: "Fetches public data from",
    events: "Receives OverCrow game events",
    storage: "Stores private widget data",
    clipboard: "Writes to clipboard on request",
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
    http: "Récupère des données publiques depuis",
    events: "Reçoit les événements de jeu OverCrow",
    storage: "Stocke des données privées du widget",
    clipboard: "Écrit dans le presse-papiers uniquement sur demande",
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
  if (!string(value, 2048)) return false;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:"
      && !parsed.username
      && !parsed.password;
  } catch {
    return false;
  }
}

function htmlEntrypoint(value) {
  return typeof value === "string"
    && value.length > 0
    && value.length <= 256
    && !value.startsWith("/")
    && !value.includes("\\")
    && !value.split("/").includes("..")
    && value.endsWith(".html");
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

function networkGrant(value) {
  return value
    && exactKeys(value, ["origin", "method", "pathPrefix"])
    && httpsUrl(value.origin)
    && ["GET", "POST", "PUT", "PATCH", "DELETE"].includes(value.method)
    && typeof value.pathPrefix === "string"
    && value.pathPrefix.startsWith("/")
    && value.pathPrefix.length <= 256;
}

function permissions(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  if (keys.some((key) => !["network", "gameEvents", "storage", "clipboardWrite"].includes(key))) {
    return false;
  }
  const network = value.network ?? [];
  const events = value.gameEvents ?? [];
  if (!Array.isArray(network) || network.length > MAX_NETWORK_GRANTS) return false;
  if (new Set(network.map((grant) => JSON.stringify(grant))).size !== network.length) return false;
  if (!network.every(networkGrant)) return false;
  if (!Array.isArray(events) || events.length > MAX_GAME_EVENTS) return false;
  if (new Set(events).size !== events.length
      || !events.every((event) => string(event, 64))) return false;
  return (value.storage === undefined || typeof value.storage === "boolean")
    && (value.clipboardWrite === undefined || typeof value.clipboardWrite === "boolean");
}

function files(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const paths = Object.keys(value);
  if (paths.length === 0 || paths.length > MAX_FILES) return false;
  return paths.every((path) => {
    const file = value[path];
    return htmlEntrypoint(path) || (string(path, 256) && !path.includes("\\") && !path.split("/").includes(".."))
      ? file
        && exactKeys(file, ["sha256", "bytes"])
        && digest(file.sha256)
        && Number.isSafeInteger(file.bytes)
        && file.bytes > 0
        && file.bytes <= MAX_PACKAGE_BYTES
      : false;
  });
}

function listing(value) {
  if (!value || !string(value.author, 128)
      || !string(value.spdxLicense, 64)
      || !/^[A-Za-z0-9][A-Za-z0-9.+-]{0,63}$/u.test(value.spdxLicense)
      || !httpsUrl(value.sourceUrl)
      || !locale(value.defaultLocale)
      || !Array.isArray(value.localizations)
      || value.localizations.length === 0
      || value.localizations.length > MAX_LISTING_LOCALES) return false;
  const listed = new Set();
  for (const entry of value.localizations) {
    if (!entry || !locale(entry.locale) || listed.has(entry.locale)
        || !string(entry.name, 128) || !string(entry.description, 512)) return false;
    listed.add(entry.locale);
  }
  return listed.has(value.defaultLocale);
}

function preview(value, manifest) {
  if (value === undefined || value === null) return true;
  return value
    && value.mediaType === "image/png"
    && Number.isSafeInteger(value.size)
    && value.size > 0
    && value.size <= MAX_PREVIEW_BYTES
    && digest(value.sha256)
    && value.url === immutableObjectUrl("previews", manifest, value.sha256, "png");
}

function target(value) {
  const manifest = value && value.manifest;
  const listed = value && value.listing;
  if (!value || !manifest || !listed) return false;
  if (!["verified", "security-suspended", "revoked"].includes(value.status)) return false;
  if (manifest.schemaVersion !== 1 || !extensionId(manifest.id)
      || !version(manifest.version)
      || manifest.apiVersion !== "1") return false;
  if (manifest.kind !== undefined
      || manifest.capabilities !== undefined
      || manifest.dependencies !== undefined
      || manifest.games !== undefined
      || manifest.minHostApi !== undefined
      || manifest.files?.component !== undefined) return false;
  if (!manifest.entrypoints || !htmlEntrypoint(manifest.entrypoints.view)) return false;
  if (manifest.entrypoints.controller !== undefined
      && !htmlEntrypoint(manifest.entrypoints.controller)) return false;
  if (!permissions(manifest.permissions) || !files(manifest.files)) return false;
  if (!manifest.files[manifest.entrypoints.view]) return false;
  if (manifest.entrypoints.controller
      && !manifest.files[manifest.entrypoints.controller]) return false;
  if (!listing(listed)) return false;
  return Number.isSafeInteger(value.packageSize)
    && value.packageSize > 0
    && value.packageSize <= MAX_PACKAGE_BYTES
    && digest(value.packageSha256)
    && value.packageUrl === immutableObjectUrl("packages", manifest, value.packageSha256, "ocpkg")
    && preview(value.preview, manifest);
}

function timestamp(value) {
  return string(value, 40) && Number.isFinite(Date.parse(value));
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
  return payload.targets;
}

function localized(item) {
  return item.listing.localizations.find((text) => text.locale === state.locale)
    || item.listing.localizations.find((text) => text.locale === "en")
    || item.listing.localizations.find((text) => text.locale === item.listing.defaultLocale)
    || item.listing.localizations[0];
}

function details(item) {
  const languageCopy = copy[state.locale];
  const granted = item.manifest.permissions || {};
  const values = [];
  const hosts = [...new Set((granted.network || []).map((grant) => {
    try {
      return new URL(grant.origin).host;
    } catch {
      return grant.origin;
    }
  }))];
  if (hosts.length) values.push(`${languageCopy.http}: ${hosts.join(", ")}`);
  if ((granted.gameEvents || []).length) values.push(languageCopy.events);
  if (granted.storage) values.push(languageCopy.storage);
  if (granted.clipboardWrite) values.push(languageCopy.clipboard);
  return values;
}

function textElement(tag, value) {
  const element = document.createElement(tag);
  element.textContent = value;
  return element;
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
  const locales = item.listing.localizations.map((entry) => entry.locale).join(", ");
  element.append(
    textElement("h2", text.name),
    textElement("p", text.description),
    textElement("p", `${languageCopy.version} ${item.manifest.version}`),
    textElement("p", `${languageCopy.author} ${item.listing.author}`),
    textElement("p", `${languageCopy.license} ${item.listing.spdxLicense}`),
    textElement("p", `${languageCopy.languages} ${locales}`),
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
  for (const item of state.targets) catalog.append(card(item));
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
