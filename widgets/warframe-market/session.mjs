import { parseCatalog, searchItems, normalizeQuery } from './catalog.mjs';
import { parseOrders } from './orders.mjs';

const ITEMS_URL = 'https://api.warframe.market/v2/items';
const VERSIONS_URL = 'https://api.warframe.market/v2/versions';
const CATALOG_KEY = 'catalog';

export function createMarketSession({ store, fetchJson }) {
  let items = [];
  let query = '';
  let results = [];
  let detail = null;
  let version = '';

  async function start() {
    const cached = await store.get(CATALOG_KEY);
    if (cached?.items?.length) {
      items = cached.items;
      version = cached.version ?? '';
    }
    const remoteVersion = await readVersion();
    if (!items.length || (remoteVersion && remoteVersion !== version)) {
      const payload = await fetchJson(ITEMS_URL);
      items = parseCatalog(payload);
      version = remoteVersion || version;
      await store.set(CATALOG_KEY, { version, items });
    }
  }

  async function handleView(message) {
    switch (message?.type) {
      case 'hello':
        return snapshot();
      case 'query': {
        query = normalizeQuery(message.value);
        results = query ? searchItems(items, query) : [];
        detail = null;
        return snapshot();
      }
      case 'select': {
        const selected = items.find((item) => item.slug === message.slug);
        if (!selected) {
          detail = null;
          return snapshot();
        }
        const payload = await fetchJson(`${ITEMS_URL}/${selected.slug}/orders`);
        detail = {
          name: selected.name,
          slug: selected.slug,
          orders: parseOrders(payload),
        };
        return snapshot();
      }
      default:
        return snapshot();
    }
  }

  async function readVersion() {
    try {
      const payload = await fetchJson(VERSIONS_URL);
      const value = payload?.data?.collections?.items;
      return typeof value === 'string' ? value : '';
    } catch {
      return '';
    }
  }

  function snapshot() {
    return {
      items: items.length,
      query,
      results,
      detail,
    };
  }

  return { start, handleView, snapshot };
}

export function createIndexedDbStore(databaseName = 'overcrow-warframe-market') {
  function open() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(databaseName, 1);
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains('kv')) {
          db.createObjectStore('kv');
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  }

  return {
    async get(key) {
      const db = await open();
      return new Promise((resolve, reject) => {
        const request = db.transaction('kv', 'readonly').objectStore('kv').get(key);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
    },
    async set(key, value) {
      const db = await open();
      return new Promise((resolve, reject) => {
        const request = db.transaction('kv', 'readwrite').objectStore('kv').put(value, key);
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    },
  };
}
