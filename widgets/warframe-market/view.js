import overcrow from './overcrow.js';
import { whisperLine } from './orders.mjs';

const query = document.querySelector('#query');
const status = document.querySelector('#status');
const results = document.querySelector('#results');
const detail = document.querySelector('#detail');
const detailName = document.querySelector('#detail-name');
const orders = document.querySelector('#orders');

let state = { items: 0, query: '', results: [], detail: null };

overcrow.runtime.onMessage((message) => {
  state = message;
  render();
  void overcrow.surface.invalidate();
});

overcrow.lifecycle.onVisibility((visible) => {
  document.body.dataset.visible = String(visible);
  if (visible) {
    void overcrow.runtime.send({ type: 'hello' });
  }
});

query.addEventListener('input', () => {
  void overcrow.runtime.send({ type: 'query', value: query.value });
});

results.addEventListener('click', (event) => {
  const button = event.target.closest('[data-slug]');
  if (!button) {
    return;
  }
  void overcrow.runtime.send({ type: 'select', slug: button.dataset.slug });
});

orders.addEventListener('click', async (event) => {
  const button = event.target.closest('[data-order]');
  if (!button || !state.detail) {
    return;
  }
  const order = state.detail.orders.find((entry) => entry.id === button.dataset.order);
  if (!order) {
    return;
  }
  try {
    await overcrow.clipboard.writeText(whisperLine(order, state.detail.name));
    status.textContent = 'Trade whisper copied';
  } catch (error) {
    status.textContent = `Clipboard rejected: ${error.code ?? 'unknown'}`;
  }
});

void overcrow.runtime.send({ type: 'hello' });

function render() {
  if (document.activeElement !== query) {
    query.value = state.query ?? '';
  }
  status.textContent = state.items
    ? `${state.items} items cached`
    : 'Catalog unavailable';
  results.replaceChildren(
    ...(state.results ?? []).map((item) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.slug = item.slug;
      button.textContent = item.name;
      const row = document.createElement('li');
      row.append(button);
      return row;
    }),
  );
  if (!state.detail) {
    detail.hidden = true;
    return;
  }
  detail.hidden = false;
  detailName.textContent = state.detail.name;
  orders.replaceChildren(
    ...['sell', 'buy'].flatMap((side) => {
      const heading = document.createElement('h2');
      heading.textContent = side === 'sell' ? 'Sellers' : 'Buyers';
      const list = document.createElement('ul');
      for (const order of state.detail.orders.filter((entry) => entry.side === side)) {
        const button = document.createElement('button');
        button.type = 'button';
        button.dataset.order = order.id;
        button.textContent = `${order.platinum}p · ${order.trader} · ${order.presence}`;
        const row = document.createElement('li');
        row.append(button);
        list.append(row);
      }
      return [heading, list];
    }),
  );
}
