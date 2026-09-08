import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

import { parseCatalog, searchItems } from '../../widgets/warframe-market/catalog.mjs';
import { parseOrders, whisperLine } from '../../widgets/warframe-market/orders.mjs';
import { createIndexedDbStore, createMarketSession } from '../../widgets/warframe-market/session.mjs';

const root = dirname(fileURLToPath(import.meta.url));

async function fixture(name) {
  return JSON.parse(await readFile(join(root, 'fixtures', name), 'utf8'));
}

function memoryStore() {
  const data = new Map();
  return {
    async get(key) {
      return data.has(key) ? structuredClone(data.get(key)) : undefined;
    },
    async set(key, value) {
      data.set(key, structuredClone(value));
    },
  };
}

test('catalog parser keeps structured items and searches without splitting bytes', () => {
  const items = parseCatalog({
    data: [
      { slug: 'arcane_energize', i18n: { en: { name: 'Arcane Energize' } } },
      { slug: 'primed_flow', i18n: { en: { name: 'Primed Flow' } } },
      { slug: 'forma_blueprint', i18n: { en: { name: 'Forma Blueprint' } } },
    ],
  });
  assert.equal(items.length, 3);
  assert.deepEqual(
    searchItems(items, 'arcane').map((item) => item.slug),
    ['arcane_energize'],
  );
  assert.deepEqual(
    searchItems(items, 'flow').map((item) => item.slug),
    ['primed_flow'],
  );
});

test('catalog of 3840 structured items stays searchable as whole records', () => {
  const payload = {
    data: Array.from({ length: 3840 }, (_, index) => ({
      slug: `item_${String(index).padStart(4, '0')}`,
      i18n: { en: { name: `Item ${index}` } },
    })),
  };
  payload.data[1920].i18n.en.name = 'Soma Prime Receiver';
  payload.data[1920].slug = 'soma_prime_receiver';
  const items = parseCatalog(payload);
  assert.equal(items.length, 3840);
  assert.equal(JSON.stringify(items[0]).includes('base64'), false);
  const hits = searchItems(items, 'soma prime');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].slug, 'soma_prime_receiver');
  assert.equal(hits[0].name, 'Soma Prime Receiver');
});

test('orders parser keeps PC visible top buy and sell rows and builds a whisper', async () => {
  const orders = parseOrders(await fixture('orders.json'));
  assert.ok(orders.some((order) => order.side === 'sell' && order.trader === 'SellerOne'));
  assert.ok(!orders.some((order) => order.trader === 'Hidden'));
  const sell = orders.find((order) => order.id === 'order-sell-online');
  assert.equal(
    whisperLine(sell, 'Arcane Energize'),
    '/w SellerOne Hi, WTB Arcane Energize for 100p',
  );
});

test('session keeps query and catalog across view hide/show reconnects', async () => {
  const itemsJson = await fixture('items.json');
  const fetchCalls = [];
  const session = createMarketSession({
    store: memoryStore(),
    fetchJson: async (url) => {
      fetchCalls.push(url);
      if (url.endsWith('/v2/versions')) {
        return { data: { collections: { items: 'v-test' } } };
      }
      if (url.endsWith('/v2/items')) {
        return itemsJson;
      }
      throw new Error(`unexpected ${url}`);
    },
  });

  await session.start();
  const first = await session.handleView({ type: 'hello' });
  assert.equal(first.items, 4);
  const searched = await session.handleView({ type: 'query', value: 'arcane' });
  assert.deepEqual(
    searched.results.map((item) => item.slug),
    ['arcane_energize', 'arcane_grace'],
  );

  const resumed = await session.handleView({ type: 'hello' });
  assert.equal(resumed.query, 'arcane');
  assert.deepEqual(
    resumed.results.map((item) => item.slug),
    ['arcane_energize', 'arcane_grace'],
  );
  assert.equal(fetchCalls.filter((url) => url.endsWith('/v2/items')).length, 1);
});

test('session restores the last query and structured catalog after controller restart', async () => {
  const itemsJson = await fixture('items.json');
  const store = memoryStore();
  const fetchJson = async (url) => {
    if (url.endsWith('/v2/versions')) {
      return { data: { collections: { items: 'v-test' } } };
    }
    if (url.endsWith('/v2/items')) {
      return itemsJson;
    }
    throw new Error(`unexpected ${url}`);
  };

  const first = createMarketSession({ store, fetchJson });
  await first.start();
  await first.handleView({ type: 'query', value: 'flow' });

  const restarted = createMarketSession({ store, fetchJson });
  await restarted.start();
  const state = await restarted.handleView({ type: 'hello' });
  assert.equal(state.query, 'flow');
  assert.deepEqual(state.results.map((item) => item.slug), ['primed_flow']);
});

test('session loads orders through overcrow.fetch and never calls global fetch', async () => {
  const itemsJson = await fixture('items.json');
  const ordersJson = await fixture('orders.json');
  let ambient = 0;
  const previous = globalThis.fetch;
  globalThis.fetch = async () => {
    ambient += 1;
    throw new Error('ambient fetch must not run');
  };
  try {
    const session = createMarketSession({
      store: memoryStore(),
      fetchJson: async (url) => {
        if (url.endsWith('/v2/versions')) {
          return { data: { collections: { items: 'v-test' } } };
        }
        if (url.endsWith('/v2/items')) {
          return itemsJson;
        }
        if (url === 'https://api.warframe.market/v2/orders/item/arcane_energize/top') {
          return ordersJson;
        }
        throw new Error(`unexpected ${url}`);
      },
    });
    await session.start();
    await session.handleView({ type: 'hello' });
    const detail = await session.handleView({
      type: 'select',
      slug: 'arcane_energize',
    });
    assert.equal(detail.detail.name, 'Arcane Energize');
    assert.ok(detail.detail.orders.length > 0);
    assert.equal(ambient, 0);
  } finally {
    globalThis.fetch = previous;
  }
});

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function pendingOrdersSession() {
  const items = await fixture('items.json');
  const pending = new Map();
  const session = createMarketSession({
    store: memoryStore(),
    fetchJson: async (url) => {
      if (url.endsWith('/versions')) return { data: { collections: { items: 'v1' } } };
      if (url.endsWith('/items')) return items;
      const request = deferred();
      pending.set(url.split('/').at(-2), request);
      return request.promise;
    },
  });
  await session.start();
  return { session, pending };
}

test('latest selection wins when order responses arrive in reverse order', async () => {
  const { session, pending } = await pendingOrdersSession();
  const first = session.handleView({ type: 'select', slug: 'arcane_energize' });
  const second = session.handleView({ type: 'select', slug: 'arcane_grace' });
  pending.get('arcane_grace').resolve(await fixture('orders.json'));
  assert.equal((await second).detail.slug, 'arcane_grace');
  pending.get('arcane_energize').resolve(await fixture('orders.json'));
  assert.equal((await first).detail.slug, 'arcane_grace');
  assert.equal(session.snapshot().detail.slug, 'arcane_grace');
});

test('a new query invalidates pending order results and failures', async () => {
  for (const fails of [false, true]) {
    const { session, pending } = await pendingOrdersSession();
    const selection = session.handleView({ type: 'select', slug: 'arcane_energize' });
    await session.handleView({ type: 'query', value: 'flow' });
    if (fails) pending.get('arcane_energize').reject(new Error('private remote error'));
    else pending.get('arcane_energize').resolve(await fixture('orders.json'));
    const state = await selection;
    assert.equal(state.query, 'flow');
    assert.equal(state.detail, null);
    assert.equal(state.error ?? null, null);
  }
});

test('failed catalog refresh preserves the validated cache and last query', async () => {
  const store = memoryStore();
  const items = parseCatalog(await fixture('items.json'));
  await store.set('catalog', { version: 'v1', items });
  await store.set('state', { query: 'flow' });
  const session = createMarketSession({
    store,
    fetchJson: async (url) => {
      if (url.endsWith('/versions')) return { data: { collections: { items: 'v2' } } };
      throw new Error('private network failure');
    },
  });
  await session.start();
  const state = await session.handleView({ type: 'hello' });
  assert.equal(state.items, 4);
  assert.equal(state.results[0].slug, 'primed_flow');
  assert.equal(state.error, 'catalog_unavailable');
  assert.deepEqual(await store.get('catalog'), { version: 'v1', items });
});

test('unavailable storage and network yield controlled usable session state', async () => {
  const session = createMarketSession({
    store: { async get() { throw new Error('private storage error'); }, async set() { throw new Error('private storage error'); } },
    fetchJson: async () => { throw new Error('private network error'); },
  });
  await session.start();
  const state = await session.handleView({ type: 'query', value: 'flow' });
  assert.equal(state.query, 'flow');
  assert.equal(state.items, 0);
  assert.equal(state.error, 'storage_unavailable');
});

test('orders failure clears the old detail and returns a controlled error', async () => {
  const { session, pending } = await pendingOrdersSession();
  const first = session.handleView({ type: 'select', slug: 'arcane_energize' });
  pending.get('arcane_energize').resolve(await fixture('orders.json'));
  await first;
  const second = session.handleView({ type: 'select', slug: 'arcane_grace' });
  pending.get('arcane_grace').reject(new Error('private order error'));
  const state = await second;
  assert.equal(state.detail, null);
  assert.equal(state.error, 'orders_unavailable');
});

test('cached records are bounded and revalidated before search or order requests', async () => {
  const invalidCaches = [
    { items: [{ slug: '../versions?secret', name: 'unsafe' }] },
    { items: [{ slug: 'safe', name: 42 }] },
    { items: [{ slug: 'safe', name: 'x'.repeat(4096) }] },
    { items: Array.from({ length: 8193 }, () => ({ slug: 'safe', name: 'Safe' })) },
    { items: { length: 1 } },
  ];
  for (const cached of invalidCaches) {
    const store = memoryStore();
    await store.set('catalog', cached);
    const calls = [];
    const session = createMarketSession({ store, fetchJson: async (url) => { calls.push(url); throw new Error('offline'); } });
    await session.start();
    const state = await session.handleView({ type: 'query', value: 'safe' });
    assert.equal(state.items, 0);
    assert.deepEqual(state.results, []);
    await session.handleView({ type: 'select', slug: '../versions?secret' });
    assert.equal(calls.some((url) => url.includes('secret')), false);
  }
});

// A controllable browser boundary: request success and transaction completion
// are distinct events, including when the transaction aborts after a put.
function indexedDbHarness({ automatic = false, records = new Map() } = {}) {
  const transactions = [];
  let closed = 0;
  const indexedDB = {
    open() {
      const opening = {};
      const db = {
        close() { closed += 1; },
        transaction() {
          const ready = deferred();
          const transaction = {
            ready: ready.promise,
            objectStore() {
              function requestFor(result, commit = () => {}) {
                const request = { result };
                queueMicrotask(() => {
                  request.onsuccess?.();
                  ready.resolve();
                  if (automatic) queueMicrotask(() => { commit(); transaction.oncomplete?.(); });
                });
                return request;
              }
              return {
                get(key) { return requestFor(records.get(key)); },
                put(value, key) { return requestFor(key, () => records.set(key, structuredClone(value))); },
              };
            },
          };
          transactions.push(transaction);
          return transaction;
        },
      };
      queueMicrotask(() => { opening.result = db; opening.onsuccess(); });
      return opening;
    },
  };
  return { indexedDB, transactions, closed: () => closed };
}

async function withIndexedDb(harness, run) {
  const previous = globalThis.indexedDB;
  globalThis.indexedDB = harness.indexedDB;
  try { await run(); } finally {
    if (previous === undefined) delete globalThis.indexedDB;
    else globalThis.indexedDB = previous;
  }
}

test('IndexedDB reads and writes settle only at commit and close their connections', async () => {
  for (const method of ['get', 'set']) {
    const harness = indexedDbHarness({ records: new Map([['state', { query: 'flow' }]]) });
    await withIndexedDb(harness, async () => {
      let settled = false;
      const result = createIndexedDbStore()[method]('state', { query: 'arcane' }).then((value) => { settled = true; return value; });
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(settled, false, `${method} must wait for transaction completion`);
      const transaction = harness.transactions[0];
      transaction.oncomplete();
      const value = await result;
      assert.deepEqual(value, method === 'get' ? { query: 'flow' } : undefined);
      assert.equal(harness.closed(), 1);
    });
  }
});

test('IndexedDB rejects an abort after request success and closes the connection', async () => {
  const harness = indexedDbHarness();
  await withIndexedDb(harness, async () => {
    const result = createIndexedDbStore().set('state', { query: 'flow' });
    const rejected = assert.rejects(result, /transaction aborted/);
    void rejected.catch(() => {});
    await new Promise((resolve) => setImmediate(resolve));
    const transaction = harness.transactions[0];
    transaction.error = new Error('transaction aborted');
    transaction.onabort?.();
    await rejected;
    assert.equal(harness.closed(), 1);
  });
});

test('controller publishes initial state and handles messages received during startup', async () => {
  const harness = indexedDbHarness({ automatic: true, records: new Map([
    ['state', { query: 'flow' }],
    ['catalog', { version: 'v1', items: parseCatalog(await fixture('items.json')) }],
  ]) });
  const fetching = deferred();
  const releaseVersion = deferred();
  const states = [];
  let receive;
  const moduleRoot = await mkdtemp(join(tmpdir(), 'overcrow-controller-test-'));
  const previous = globalThis.__overcrowNative;
  globalThis.__overcrowNative = {
    role: 'controller',
    subscribe(listener) { receive = listener; },
    async request(metadata) {
      if (metadata.type === 'fetch') {
        assert.equal(metadata.url, 'https://api.warframe.market/v2/versions');
        fetching.resolve();
        await releaseVersion.promise;
        return { metadata: { ok: true, status: 200 }, body: new TextEncoder().encode(JSON.stringify({ data: { collections: { items: 'v1' } } })).buffer };
      }
      if (metadata.type === 'relay') states.push(metadata.payload);
      return { metadata: { ok: true }, body: new ArrayBuffer(0) };
    },
  };
  try {
    // Match the browser's explicit module mode without relying on Node syntax detection.
    await cp(join(root, '../../widgets/warframe-market'), moduleRoot, { recursive: true });
    await writeFile(join(moduleRoot, 'package.json'), JSON.stringify({ type: 'module' }));
    await withIndexedDb(harness, async () => {
      const loading = import(pathToFileURL(join(moduleRoot, 'controller.js')).href);
      await Promise.race([fetching.promise, loading]);
      receive({ type: 'relay', source: 'view', payload: { type: 'hello' } });
      receive({ type: 'relay', source: 'view', payload: { type: 'query', value: 'arcane' } });
      releaseVersion.resolve();
      await loading;
      await new Promise((resolve) => setImmediate(resolve));
      assert.ok(states.length >= 3, 'startup and both pending messages must publish state');
      assert.equal(states.at(-1).query, 'arcane');
      assert.deepEqual(states.at(-1).results.map((item) => item.slug), ['arcane_energize', 'arcane_grace']);
    });
  } finally {
    if (previous === undefined) delete globalThis.__overcrowNative;
    else globalThis.__overcrowNative = previous;
    await rm(moduleRoot, { recursive: true, force: true });
  }
});

// Exercise the view against the real SDK, with only the browser DOM and native
// request boundary controlled here. Layout is checked separately in a browser.
async function withMarketView(run) {
  const nodes = new Map();
  class Element {
    constructor(tagName = 'div') {
      this.tagName = tagName;
      this.children = [];
      this.dataset = {};
      this.attributes = {};
      this.listeners = new Map();
      this.value = '';
      this.hidden = false;
      this.disabled = false;
    }
    set textContent(value) { this.text = String(value); this.children = []; }
    get textContent() { return (this.text ?? '') + this.children.map((child) => child.textContent).join(''); }
    set innerHTML(_) { throw new Error('Provider content must be rendered as text'); }
    append(...children) { for (const child of children) { child.parentElement = this; this.children.push(child); } }
    replaceChildren(...children) { this.text = ''; this.children = []; this.append(...children); }
    setAttribute(name, value) { this.attributes[name] = String(value); }
    removeAttribute(name) { delete this.attributes[name]; }
    addEventListener(name, listener) { this.listeners.set(name, listener); }
    closest(selector) {
      const key = /^\[data-([a-z]+)\]$/.exec(selector)?.[1];
      return key && this.dataset[key] !== undefined ? this : this.parentElement?.closest(selector);
    }
    contains(target) { return target === this || this.children.some((child) => child.contains(target)); }
    async fire(name, target = this) { return this.listeners.get(name)?.({ target }); }
    focus() {
      document.activeElement?.fire('blur');
      document.activeElement = this;
      void this.fire('focus');
    }
  }
  const source = join(root, '../../widgets/warframe-market');
  const html = await readFile(join(source, 'index.html'), 'utf8');
  for (const [, id] of html.matchAll(/\bid="([a-z-]+)"/g)) nodes.set(`#${id}`, new Element());
  const document = {
    body: new Element('body'),
    activeElement: null,
    querySelector(selector) { return nodes.get(selector); },
    createElement(tagName) { return new Element(tagName); },
  };
  const moduleRoot = await mkdtemp(join(tmpdir(), 'overcrow-market-view-test-'));
  const previousDocument = globalThis.document;
  const previousNative = globalThis.__overcrowNative;
  const requests = [];
  const copies = [];
  let receive;
  globalThis.document = document;
  globalThis.__overcrowNative = {
    role: 'view',
    subscribe(listener) { receive = listener; },
    async request(metadata) {
      requests.push(metadata);
      if (metadata.type === 'clipboardWrite') {
        const pending = deferred();
        copies.push({ text: metadata.text, ...pending });
        return pending.promise;
      }
      return { metadata: { ok: true }, body: new ArrayBuffer(0) };
    },
  };
  const tick = () => new Promise((resolve) => setImmediate(resolve));
  const all = (node) => [node, ...node.children.flatMap(all)];
  try {
    await cp(source, moduleRoot, { recursive: true });
    await writeFile(join(moduleRoot, 'package.json'), JSON.stringify({ type: 'module' }));
    await import(pathToFileURL(join(moduleRoot, 'view.js')).href);
    await run({
      node: (id) => nodes.get(`#${id}`),
      buttons: () => all(nodes.get('#orders')).filter((node) => node.dataset.order),
      message: (payload) => receive({ type: 'relay', source: 'controller', payload }),
      document, requests, copies, tick,
    });
  } finally {
    if (previousDocument === undefined) delete globalThis.document;
    else globalThis.document = previousDocument;
    if (previousNative === undefined) delete globalThis.__overcrowNative;
    else globalThis.__overcrowNative = previousNative;
    await rm(moduleRoot, { recursive: true, force: true });
  }
}

async function viewSnapshot() {
  return {
    items: 3840, query: 'arcane', results: [], error: null,
    detail: { name: 'Arcane Energize', slug: 'arcane_energize', orders: parseOrders(await fixture('orders.json')) },
  };
}

test('view only confirms an explicit whisper copy after native acknowledgement', async () => {
  const snapshot = await viewSnapshot();
  await withMarketView(async ({ node, message, buttons, copies, tick }) => {
    message(snapshot);
    const button = buttons()[0];
    assert.match(button.textContent, /Copy whisper/);
    await node('orders').fire('click', button.parentElement);
    assert.equal(copies.length, 0, 'clicking the offer itself must not copy');
    const copying = node('orders').fire('click', button);
    await tick();
    assert.equal(copies[0].text, '/w SellerOne Hi, WTB Arcane Energize for 100p');
    assert.match(button.textContent, /Copying/);
    assert.ok(buttons().every((entry) => entry.disabled), 'pending native work must not accept another copy');
    assert.doesNotMatch(node('copy-status').textContent, /copied/i);
    copies[0].resolve({ metadata: { ok: true }, body: new ArrayBuffer(0) });
    await copying;
    assert.match(button.textContent, /Copied/);
    assert.match(node('copy-status').textContent, /copied/i);
    assert.ok(buttons().every((entry) => !entry.disabled));
  });
});

test('view shows copy failure, supports retry, and ignores completion after a new search', async () => {
  const snapshot = await viewSnapshot();
  await withMarketView(async ({ node, message, buttons, copies, tick, requests }) => {
    message(snapshot);
    const copying = node('orders').fire('click', buttons()[0]);
    await tick();
    copies[0].resolve({ metadata: { ok: false, error: { code: 'clipboard_failed', message: 'private native detail' } }, body: new ArrayBuffer(0) });
    await copying;
    assert.match(node('copy-status').textContent, /failed/i);
    assert.doesNotMatch(node('copy-status').textContent, /private native detail/);
    assert.equal(buttons()[0].disabled, false);
    const retry = node('orders').fire('click', buttons()[0]);
    await tick();
    node('query').value = 'flow';
    await node('query').fire('input');
    assert.ok(requests.some((request) => request.payload?.type === 'query' && request.payload.value === 'flow'));
    copies[1].resolve({ metadata: { ok: true }, body: new ArrayBuffer(0) });
    await retry;
    assert.doesNotMatch(node('copy-status').textContent, /copied/i, 'old item success must not be attributed to the new query');
  });
});

test('view derives displayed offer metrics and gives search focus back after clearing', async () => {
  const snapshot = await viewSnapshot();
  await withMarketView(async ({ node, message, document, requests }) => {
    message(snapshot);
    assert.equal(node('min-sell').textContent, '90p');
    assert.equal(node('max-buy').textContent, '85p');
    assert.equal(node('order-count').textContent, '4');
    node('query').focus();
    assert.match(node('search-hint').textContent, /Ready to type/);
    await node('clear-query').fire('click');
    assert.equal(document.activeElement, node('query'));
    assert.equal(node('query').value, '');
    assert.ok(requests.some((request) => request.payload?.type === 'query' && request.payload.value === ''));
    message({ ...snapshot, detail: { ...snapshot.detail, orders: [] } });
    assert.equal(node('min-sell').textContent, '—');
    assert.equal(node('max-buy').textContent, '—');
    assert.equal(node('order-count').textContent, '0');
    assert.match(node('orders').textContent, /No sell offers/);
    assert.match(node('orders').textContent, /No buy offers/);
  });
});
