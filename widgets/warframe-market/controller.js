import overcrow from './overcrow.js';
import { createIndexedDbStore, createMarketSession } from './session.mjs';

async function fetchJson(url) {
  const response = await overcrow.fetch(url, { method: 'GET' });
  if (!response.ok) {
    throw new Error('request failed');
  }
  return response.json();
}

const session = createMarketSession({
  store: createIndexedDbStore(),
  fetchJson,
});

await session.start();

overcrow.runtime.onMessage(async (message) => {
  const state = await session.handleView(message);
  await overcrow.runtime.send(state);
  await overcrow.surface.invalidate();
});
