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

const ready = session.start();

async function publish() {
  try {
    await overcrow.runtime.send(session.snapshot());
    await overcrow.surface.invalidate();
  } catch {
    // A hidden or disconnected view can reconnect with hello.
  }
}

overcrow.runtime.onMessage(async (message) => {
  await ready;
  await session.handleView(message);
  await publish();
});

await ready;
await publish();
