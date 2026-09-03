const MAX_ITEMS = 8192;
const MAX_NAME_CHARS = 96;
const MAX_SLUG_BYTES = 96;
const MAX_QUERY_CHARS = 64;
const MAX_RESULTS = 12;

export function parseCatalog(payload) {
  const rows = payload?.data;
  if (!Array.isArray(rows) || rows.length === 0 || rows.length > MAX_ITEMS) {
    throw new Error('invalid catalog');
  }
  const bySlug = new Map();
  for (const row of rows) {
    const item = parseItem(row);
    if (!item) {
      continue;
    }
    bySlug.set(item.slug, item);
  }
  if (bySlug.size === 0) {
    throw new Error('invalid catalog');
  }
  return [...bySlug.values()];
}

export function searchItems(items, query) {
  const normalized = normalizeQuery(query);
  if (!normalized) {
    return [];
  }
  const needle = normalized.toLowerCase();
  const hits = [];
  for (const item of items) {
    if (item.name.toLowerCase().includes(needle) || item.slug.includes(needle)) {
      hits.push(item);
      if (hits.length === MAX_RESULTS) {
        break;
      }
    }
  }
  return hits;
}

export function normalizeQuery(value) {
  if (typeof value !== 'string') {
    return '';
  }
  const trimmed = value.trim();
  if (trimmed.length > MAX_QUERY_CHARS || [...trimmed].some((character) => character < ' ')) {
    return '';
  }
  return trimmed;
}

function parseItem(row) {
  if (!row || typeof row !== 'object') {
    return null;
  }
  const slug = typeof row.slug === 'string' ? row.slug : '';
  if (!safeSlug(slug)) {
    return null;
  }
  const name = sanitizeName(row.i18n?.en?.name);
  if (!name) {
    return null;
  }
  return { slug, name };
}

function safeSlug(value) {
  return (
    value.length >= 1
    && value.length <= MAX_SLUG_BYTES
    && /^[a-z0-9_-]+$/.test(value)
  );
}

function sanitizeName(value) {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  if (!trimmed || [...trimmed].some((character) => character < ' ')) {
    return null;
  }
  if ([...trimmed].length <= MAX_NAME_CHARS) {
    return trimmed;
  }
  return `${[...trimmed].slice(0, MAX_NAME_CHARS - 1).join('')}…`;
}
