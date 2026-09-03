import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { parseCatalog, searchItems } from '../../widgets/warframe-market/catalog.mjs';
import { parseOrders, whisperLine } from '../../widgets/warframe-market/orders.mjs';
import { createMarketSession } from '../../widgets/warframe-market/session.mjs';

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
        if (url.includes('/v2/items/arcane_energize/orders')) {
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
