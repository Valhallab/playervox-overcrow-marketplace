"use strict";

const MAX_ENVELOPE_BYTES = 1024 * 1024;
const MAX_STREAM_CHUNKS = 4096;
const MAX_TARGETS = 500;
const MAX_PACKAGE_BYTES = 128 * 1024 * 1024;
const MAX_PREVIEW_BYTES = 256 * 1024;
const MAX_LISTING_LOCALES = 16;
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

const state = { locale: "en", targets: [], unavailable: false, loading: true, query: "", availability: "all" };
const catalog = document.getElementById("catalog");
const language = document.getElementById("language");
const trust = document.getElementById("trust-label");
const search = document.getElementById("search");
const availability = document.getElementById("availability");
const detail = document.getElementById("detail");
const copy = {
  en: {
    title: "Your game. Your widgets.",
    description: "Useful companions, one overlay away. Discover widgets, install them in Control Center, and make them yours in game.",
    language: "Language",
    catalogHeading: "Explore widgets",
    browse: "Browse widgets", create: "Create a widget ↗",
    kicker: "THE OVERCROW MARKETPLACE", flowTitle: "FROM DISCOVERY TO PLAY",
    flowBrowse: "Find your widget", flowInstall: "Install in Control Center",
    flowActivate: "Activate in your in-game overlay",
    search: "Search widgets", searchHint: "Name, description, author…",
    availability: "Availability", all: "All widgets", available: "Available", restricted: "Restricted",
    reset: "Clear filters", noResults: "No matching widgets. Try another search or clear your filters.",
    count: (shown, total) => shown === total ? `${total} widget${total === 1 ? "" : "s"}` : `${shown} / ${total} widgets`,
    view: "View widget", back: "← All widgets", notFound: "Widget not found in this catalog.",
    installTitle: "Bring it into your game", about: "About this widget",
    footer: "Built for your overlay. Always outside your game.",
    previewFallback: "WEB WIDGET",
    skip: "Skip to widgets",
    loading: "Loading widgets…",
    empty: "No widgets are listed yet.",
    permissions: "Permissions",
    noPermissions: "No permissions requested.",
    unavailable: "Catalog unavailable.",
    version: "Version",
    author: "Author",
    source: "View source",
    open: "Open in OverCrow",
    openHelp: "Requires the OverCrow app. You can also open Control Center and find this widget in Marketplace. Opening its details does not install or activate it.",
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
    title: "Votre jeu. Vos widgets.",
    description: "Vos compagnons de jeu, à portée d’overlay. Découvrez vos widgets, installez-les dans le Centre de contrôle OverCrow et activez-les en jeu.",
    language: "Langue",
    catalogHeading: "Explorez les widgets",
    browse: "Explorer", create: "Créer un widget ↗",
    kicker: "LA MARKETPLACE OVERCROW", flowTitle: "DE LA DÉCOUVERTE AU JEU",
    flowBrowse: "Trouvez votre widget", flowInstall: "Installez-le dans le Centre de contrôle",
    flowActivate: "Activez-le dans votre overlay en jeu",
    search: "Rechercher un widget", searchHint: "Nom, description, auteur…",
    availability: "Disponibilité", all: "Tous les widgets", available: "Disponibles", restricted: "Restreints",
    reset: "Effacer les filtres", noResults: "Aucun widget correspondant. Essayez une autre recherche ou effacez les filtres.",
    count: (shown, total) => shown === total ? `${total} widget${total === 1 ? "" : "s"}` : `${shown} / ${total} widgets`,
    view: "Voir le widget", back: "← Tous les widgets", notFound: "Widget introuvable dans ce catalogue.",
    installTitle: "Retrouvez-le en jeu", about: "À propos du widget",
    footer: "Pensé pour votre overlay. Toujours en dehors du jeu.",
    previewFallback: "WIDGET WEB",
    skip: "Aller aux widgets",
    loading: "Chargement des widgets…",
    empty: "Aucun widget n’est encore proposé.",
    permissions: "Permissions",
    noPermissions: "Aucune permission demandée.",
    unavailable: "Catalogue indisponible.",
    version: "Version",
    author: "Auteur",
    source: "Voir le code source",
    open: "Ouvrir dans OverCrow",
    openHelp: "Nécessite l’application OverCrow. Vous pouvez aussi ouvrir le Centre de contrôle et retrouver ce widget dans Marketplace. Ouvrir sa fiche ne l’installe ni ne l’active.",
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

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareDigits(left, right) {
  return left.length - right.length || compareText(left, right);
}

function parseVersion(value) {
  if (!string(value, 64)) return null;
  const parts = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/u.exec(value);
  if (!parts) return null;
  const core = parts.slice(1, 4);
  const pre = parts[4] ? parts[4].split(".") : [];
  const build = parts[5] ? parts[5].split(".") : [];
  // Rust semver uses u64 core numbers and rejects leading-zero numeric prereleases.
  if (core.some((part) => compareDigits(part, "18446744073709551615") > 0)
      || pre.some((part) => /^0[0-9]+$/u.test(part))) return null;
  return { core, pre, build };
}

function version(value) {
  return parseVersion(value) !== null;
}

function compareIdentifiers(left, right, build) {
  for (let index = 0; index < Math.min(left.length, right.length); index += 1) {
    const a = left[index];
    const b = right[index];
    const aNumeric = /^[0-9]+$/u.test(a);
    const bNumeric = /^[0-9]+$/u.test(b);
    let result;
    if (aNumeric && bNumeric) {
      // Version::cmp includes build metadata: equal numeric values sort by digit count.
      result = build
        ? compareDigits(a.replace(/^0+/u, ""), b.replace(/^0+/u, "")) || a.length - b.length
        : compareDigits(a, b);
    } else {
      result = aNumeric !== bNumeric ? (aNumeric ? -1 : 1) : compareText(a, b);
    }
    if (result) return result;
  }
  return left.length - right.length;
}

function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    const result = compareDigits(a.core[index], b.core[index]);
    if (result) return result;
  }
  if (a.pre.length === 0 && b.pre.length !== 0) return 1;
  if (b.pre.length === 0 && a.pre.length !== 0) return -1;
  return compareIdentifiers(a.pre, b.pre, false)
    || compareIdentifiers(a.build, b.build, true);
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
  // The admitted manifest bounds these collections by bytes, not by grant count.
  if (!Array.isArray(network)) return false;
  if (new Set(network.map((grant) => JSON.stringify(grant))).size !== network.length) return false;
  if (!network.every(networkGrant)) return false;
  if (!Array.isArray(events)) return false;
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
      || !Array.isArray(payload.targets)
      || payload.targets.length > MAX_TARGETS || !payload.targets.every(target)) {
    throw new Error("payload");
  }
  const byId = new Map();
  const identities = new Set();
  for (const item of payload.targets) {
    const { id, version } = item.manifest;
    const identity = `${id}/${version}`;
    if (identities.has(identity)) throw new Error("target");
    identities.add(identity);
    const previous = byId.get(id);
    // Select before considering status so a revocation never reveals an older version.
    if (!previous || compareVersions(version, previous.manifest.version) > 0) {
      byId.set(id, item);
    }
  }
  return [...byId.values()];
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

function textElement(tag, value, className) {
  const element = document.createElement(tag);
  element.textContent = value;
  if (className) element.setAttribute("class", className);
  return element;
}

function statusLabel(status, languageCopy) {
  if (status === "verified") return languageCopy.verified;
  if (status === "security-suspended") return languageCopy.suspended;
  return languageCopy.revoked;
}

function widgetPreview(item) {
  const media = document.createElement("div");
  media.setAttribute("class", "widget-preview");
  if (item.preview) {
    const image = document.createElement("img");
    image.setAttribute("src", item.preview.url);
    image.setAttribute("alt", "");
    image.setAttribute("loading", "lazy");
    media.append(image);
  } else {
    media.setAttribute("class", "widget-preview widget-preview--fallback");
    media.setAttribute("aria-hidden", "true");
    media.append(textElement("span", localized(item).name.slice(0, 2).toLocaleUpperCase(state.locale), "widget-monogram"),
      textElement("span", copy[state.locale].previewFallback, "preview-caption"));
  }
  return media;
}

function card(item) {
  const text = localized(item);
  const words = copy[state.locale];
  const element = document.createElement("article");
  element.setAttribute("class", "card");
  const link = document.createElement("a");
  link.setAttribute("class", "card-link");
  link.setAttribute("href", `#widget/${item.manifest.id}`);
  link.setAttribute("aria-label", `${words.view}: ${text.name}`);
  link.setAttribute("aria-describedby", `description-${item.manifest.id} status-${item.manifest.id}`);
  const body = document.createElement("div");
  body.setAttribute("class", "card-body");
  const description = textElement("p", text.description, "description");
  description.setAttribute("id", `description-${item.manifest.id}`);
  body.append(textElement("h3", text.name), textElement("p", item.listing.author, "card-author"), description);
  const footer = document.createElement("div");
  footer.setAttribute("class", "card-footer");
  footer.append(textElement("span", `${words.version} ${item.manifest.version}`), textElement("span", `${words.view} →`, "card-action"));
  const status = textElement("p", statusLabel(item.status, words), `badge status-${item.status}`);
  status.setAttribute("id", `status-${item.manifest.id}`);
  body.append(status, footer);
  link.append(widgetPreview(item), body);
  element.append(link);
  return element;
}

function widgetDetails(item) {
  const text = localized(item);
  const words = copy[state.locale];
  const layout = document.createElement("div");
  layout.setAttribute("class", "detail-layout");
  const main = document.createElement("article");
  main.setAttribute("class", "detail-main");
  main.append(textElement("p", item.listing.author, "eyebrow"), textElement("h1", text.name),
    textElement("p", text.description, "detail-description"), widgetPreview(item));
  const permissions = document.createElement("section");
  permissions.setAttribute("class", "permissions");
  permissions.append(textElement("h2", words.permissions));
  const values = details(item);
  if (values.length) {
    const list = document.createElement("ul");
    for (const value of values) list.append(textElement("li", value));
    permissions.append(list);
  } else permissions.append(textElement("p", words.noPermissions));
  main.append(permissions);
  const aside = document.createElement("aside");
  aside.setAttribute("class", "detail-sidebar");
  const install = document.createElement("section");
  install.setAttribute("class", "install-panel");
  const open = document.createElement("a");
  open.textContent = words.open;
  open.setAttribute("href", `overcrow://widget/${item.manifest.id}`);
  open.setAttribute("class", "button-primary");
  install.append(textElement("h2", words.installTitle),
    textElement("p", statusLabel(item.status, words), `badge status-${item.status}`),
    open, textElement("p", words.openHelp, "open-help"), textElement("p", words.flowActivate, "activation-note"));
  const metadata = document.createElement("section");
  metadata.setAttribute("class", "metadata");
  metadata.append(textElement("h2", words.about));
  for (const [label, value] of [[words.version, item.manifest.version], [words.author, item.listing.author],
    [words.license, item.listing.spdxLicense], [words.languages, item.listing.localizations.map((entry) => entry.locale).join(", ")]]) {
    metadata.append(textElement("p", `${label} ${value}`));
  }
  metadata.append(textElement("p", item.manifest.id, "widget-id"));
  const source = document.createElement("a");
  source.textContent = words.source;
  source.setAttribute("href", item.listing.sourceUrl);
  source.setAttribute("title", item.listing.sourceUrl);
  source.setAttribute("rel", "noreferrer noopener");
  source.setAttribute("class", "source-link");
  metadata.append(source);
  aside.append(install, metadata);
  layout.append(main, aside);
  return layout;
}

function renderRoute(focus = false) {
  const isDetail = location.hash.startsWith("#widget/");
  let focusTarget = document.getElementById("catalog-heading");
  document.getElementById("catalog-view").hidden = isDetail;
  detail.hidden = !isDetail;
  detail.textContent = "";
  const words = copy[state.locale];
  document.title = `${words.title} · OverCrow`;
  if (isDetail) {
    const back = document.createElement("a");
    back.setAttribute("href", "#catalog-area");
    back.setAttribute("class", "back-link");
    back.textContent = words.back;
    detail.append(back);
    focusTarget = back;
    const item = state.targets.find((target) => `#widget/${target.manifest.id}` === location.hash);
    if (item) {
      detail.append(widgetDetails(item));
      document.title = `${localized(item).name} · OverCrow`;
    } else detail.append(textElement("h1", state.loading ? words.loading : state.unavailable ? words.unavailable : words.notFound, "catalog-state"));
  }
  if (focus) focusTarget.focus();
}

function renderCatalog() {
  catalog.textContent = "";
  document.getElementById("catalog-status").textContent = "";
  const words = copy[state.locale];
  const normalize = (value) => value.normalize("NFD").replace(/[\u0300-\u036f]/gu, "").toLocaleLowerCase(state.locale);
  const query = normalize(state.query.trim());
  const visible = state.targets.filter((item) => {
    const matchesStatus = state.availability === "all" || (state.availability === "available" ? item.status === "verified" : item.status !== "verified");
    return matchesStatus && normalize([item.manifest.id, item.listing.author,
      ...item.listing.localizations.flatMap((entry) => [entry.name, entry.description])].join(" ")).includes(query);
  });
  document.getElementById("result-count").textContent = state.loading || state.unavailable ? "" : words.count(visible.length, state.targets.length);
  document.getElementById("reset-search").hidden = !state.query && state.availability === "all";
  catalog.setAttribute("aria-busy", String(state.loading));
  if (state.loading || state.unavailable || !visible.length) {
    const message = state.loading ? words.loading : state.unavailable ? words.unavailable
      : state.targets.length ? words.noResults : words.empty;
    document.getElementById("catalog-status").textContent = message;
    const messageElement = textElement("p", message, "catalog-state");
    messageElement.setAttribute("aria-hidden", "true");
    catalog.append(messageElement);
  } else for (const item of visible) catalog.append(card(item));
}

function render() {
  const words = copy[state.locale];
  for (const [id, text] of [
    ["page-title", words.title], ["page-description", words.description], ["language-label", words.language],
    ["catalog-heading", words.catalogHeading], ["skip-link", words.skip], ["nav-browse", words.browse],
    ["nav-create", words.create], ["hero-kicker", words.kicker], ["flow-title", words.flowTitle],
    ["flow-browse", words.flowBrowse], ["flow-install", words.flowInstall], ["flow-activate", words.flowActivate],
    ["search-label", words.search], ["availability-label", words.availability], ["filter-all", words.all],
    ["filter-available", words.available], ["filter-restricted", words.restricted], ["reset-search", words.reset],
    ["footer-note", words.footer],
  ]) document.getElementById(id).textContent = text;
  search.setAttribute("placeholder", words.searchHint);
  language.setAttribute("aria-label", words.language);
  trust.textContent = policy.labels[state.locale];
  renderCatalog();
  renderRoute();
}

search.addEventListener("input", () => {
  state.query = search.value.slice(0, 200);
  renderCatalog();
});
availability.addEventListener("change", () => {
  state.availability = ["available", "restricted"].includes(availability.value) ? availability.value : "all";
  renderCatalog();
});
document.getElementById("reset-search").addEventListener("click", () => {
  state.query = "";
  state.availability = "all";
  search.value = "";
  availability.value = "all";
  renderCatalog();
  search.focus();
});
addEventListener("hashchange", () => renderRoute(true));

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

render();

fetch(policy.catalogUrl)
  .then((response) => {
    if (!response.ok) throw new Error("catalog unavailable");
    return readBounded(response);
  })
  .then((text) => {
    state.targets = validate(text);
    state.loading = false;
    render();
  })
  .catch(() => {
    state.loading = false;
    state.unavailable = true;
    render();
  });
