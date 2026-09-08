import { parseCatalog, searchItems, normalizeQuery } from './catalog.mjs';
import { parseOrders } from './orders.mjs';

const ITEMS_URL = 'https://api.warframe.market/v2/items';
const VERSIONS_URL = 'https://api.warframe.market/v2/versions';
const CATALOG_KEY = 'catalog';
const STATE_KEY = 'state';

export function createMarketSession({ store, fetchJson }) {
  let items = [];
  let query = '';
  let results = [];
  let detail = null;
  let version = '';
  let error = null;
  let action = null;

  async function start() {
    const savedState = await readStored(STATE_KEY);
    query = normalizeQuery(savedState?.query);
    const cached = await readStored(CATALOG_KEY);
    // Stored data crosses the same validation boundary as remote catalog data.
    if (Array.isArray(cached?.items) && cached.items.length <= 8192) {
      try {
        items = parseCatalog({ data: cached.items.map((item) => ({
          slug: item?.slug,
          i18n: { en: { name: typeof item?.name === 'string' && item.name.length <= 192 ? item.name : null } },
        })) });
        version = validVersion(cached.version);
      } catch {
        // An invalid cache cannot authorize search results or request paths.
      }
    }
    const remoteVersion = await readVersion();
    if (!items.length || (remoteVersion && remoteVersion !== version)) {
      try {
        const payload = await fetchJson(ITEMS_URL);
        items = parseCatalog(payload);
        version = remoteVersion || version;
      } catch {
        error = 'catalog_unavailable';
        results = query ? searchItems(items, query) : [];
        return;
      }
      try {
        await store.set(CATALOG_KEY, { version, items });
      } catch {
        error = 'storage_unavailable';
      }
    }
    results = query ? searchItems(items, query) : [];
  }

  async function handleView(message) {
    switch (message?.type) {
      case 'hello':
        return snapshot();
      case 'query': {
        const current = action = {};
        error = null;
        query = normalizeQuery(message.value);
        results = query ? searchItems(items, query) : [];
        detail = null;
        try {
          await store.set(STATE_KEY, { query });
        } catch {
          if (action === current) error = 'storage_unavailable';
        }
        return snapshot();
      }
      case 'select': {
        const current = action = {};
        detail = null;
        error = null;
        const selected = items.find((item) => item.slug === message.slug);
        if (!selected) {
          return snapshot();
        }
        try {
          const payload = await fetchJson(`https://api.warframe.market/v2/orders/item/${selected.slug}/top`);
          if (action === current) {
            detail = {
              name: selected.name,
              slug: selected.slug,
              orders: parseOrders(payload),
            };
          }
        } catch {
          if (action === current) error = 'orders_unavailable';
        }
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
      return validVersion(value);
    } catch {
      return '';
    }
  }

  async function readStored(key) {
    try {
      return await store.get(key);
    } catch {
      error = 'storage_unavailable';
      return undefined;
    }
  }

  function validVersion(value) {
    return typeof value === 'string' && value.length <= 128 ? value : '';
  }

  function snapshot() {
    return {
      items: items.length,
      query,
      results,
      detail,
      error,
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

  async function transact(mode, operation) {
    const db = await open();
    try {
      return await new Promise((resolve, reject) => {
        const transaction = db.transaction('kv', mode);
        const request = operation(transaction.objectStore('kv'));
        transaction.oncomplete = () => resolve(mode === 'readonly' ? request.result : undefined);
        transaction.onabort = () => reject(transaction.error ?? new Error('transaction aborted'));
        transaction.onerror = () => reject(transaction.error ?? request.error ?? new Error('transaction failed'));
      });
    } finally {
      db.close();
    }
  }

  return {
    get(key) {
      return transact('readonly', (store) => store.get(key));
    },
    set(key, value) {
      return transact('readwrite', (store) => store.put(value, key));
    },
  };
}
