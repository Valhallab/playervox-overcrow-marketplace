import overcrow from './overcrow.js';
import { whisperLine } from './orders.mjs';

const query = document.querySelector('#query');
const clearQuery = document.querySelector('#clear-query');
const searchHint = document.querySelector('#search-hint');
const status = document.querySelector('#status');
const results = document.querySelector('#results');
const detail = document.querySelector('#detail');
const detailName = document.querySelector('#detail-name');
const orders = document.querySelector('#orders');
const emptyState = document.querySelector('#empty-state');
const copyStatus = document.querySelector('#copy-status');
const minSell = document.querySelector('#min-sell');
const maxBuy = document.querySelector('#max-buy');
const orderCount = document.querySelector('#order-count');

let state = { items: 0, query: '', results: [], detail: null };
let detailKey = '';
let copyContext = {};
let copyFeedback = null;
let copying = false;
let copyButtons = [];

function invalidate() {
  void overcrow.surface.invalidate().catch(() => {});
}

async function send(message) {
  try {
    await overcrow.runtime.send(message);
  } catch {
    status.textContent = 'Connection unavailable · reopen the widget to retry';
    status.dataset.error = 'true';
    invalidate();
  }
}

overcrow.runtime.onMessage((message) => {
  state = message;
  render();
  invalidate();
});

overcrow.lifecycle.onVisibility((visible) => {
  document.body.dataset.visible = String(visible);
  if (visible) void send({ type: 'hello' });
});

query.addEventListener('focus', () => {
  searchHint.textContent = 'Ready to type · results update as you type';
  invalidate();
});
query.addEventListener('blur', () => {
  searchHint.textContent = 'Click search to type · results update as you type';
  invalidate();
});
query.addEventListener('input', search);
clearQuery.addEventListener('click', () => {
  query.value = '';
  query.focus();
  search();
});

function search() {
  resetCopyFeedback();
  state = { ...state, query: query.value, results: [], detail: null, error: null };
  render();
  if (query.value.trim()) {
    status.textContent = 'Searching catalog…';
    emptyState.textContent = 'Searching cached items…';
  }
  void send({ type: 'query', value: query.value });
  invalidate();
}

results.addEventListener('click', (event) => {
  const button = event.target.closest('[data-slug]');
  if (!button || !results.contains(button)) return;
  resetCopyFeedback();
  state = { ...state, detail: null, error: null };
  render();
  status.textContent = 'Loading offers…';
  void send({ type: 'select', slug: button.dataset.slug });
  invalidate();
});

orders.addEventListener('click', async (event) => {
  const button = event.target.closest('[data-order]');
  if (!button || !orders.contains(button) || !state.detail || copying) return;
  const order = state.detail.orders.find((entry) => entry.id === button.dataset.order && entry.side === button.dataset.side);
  if (!order) return;
  const context = copyContext;
  const feedback = { id: order.id, side: order.side, state: 'pending' };
  copyFeedback = feedback;
  copying = true;
  renderCopyFeedback();
  invalidate();
  try {
    await overcrow.clipboard.writeText(whisperLine(order, state.detail.name));
    if (copyContext === context) feedback.state = 'copied';
  } catch {
    if (copyContext === context) feedback.state = 'failed';
  } finally {
    // A reply for a previous search/selection cannot mark a different offer copied.
    if (copyContext !== context) copyFeedback = null;
    copying = false;
    renderCopyFeedback();
    invalidate();
  }
});

void send({ type: 'hello' });

function resetCopyFeedback() {
  copyContext = {};
  copyFeedback = null;
}

function render() {
  if (document.activeElement !== query) query.value = state.query ?? '';
  clearQuery.disabled = !query.value;
  const errors = {
    catalog_unavailable: state.items ? 'Catalog refresh unavailable · using cached items' : 'Catalog unavailable',
    storage_unavailable: 'Storage unavailable · changes may not survive restart',
    orders_unavailable: 'Orders unavailable · select an item to retry',
  };
  status.dataset.error = String(Boolean(errors[state.error]));
  status.textContent = errors[state.error] ?? (state.items
    ? `${state.items.toLocaleString('en-US')} items cached · ${state.detail ? 'up to 5 offers per side' : 'select an item to view offers'}`
    : 'Catalog unavailable');
  results.hidden = Boolean(state.detail) || !(state.results?.length);
  results.replaceChildren(...(state.results ?? []).map((item) => {
    const button = element('button', 'result-button');
    button.type = 'button';
    button.dataset.slug = item.slug;
    button.append(element('span', '', item.name));
    const arrow = element('span', 'result-arrow', '›');
    arrow.setAttribute('aria-hidden', 'true');
    button.append(arrow);
    const row = element('li');
    row.append(button);
    return row;
  }));
  emptyState.hidden = Boolean(state.detail) || Boolean(state.results?.length);
  emptyState.textContent = state.query
    ? 'No matching items. Try a different name.'
    : 'Find an item to compare offers and copy a trade whisper.';
  detail.hidden = !state.detail;
  const nextKey = JSON.stringify(state.detail);
  if (nextKey !== detailKey) {
    detailKey = nextKey;
    resetCopyFeedback();
    copyButtons = [];
    orders.replaceChildren();
    if (state.detail) renderDetail();
  }
  renderCopyFeedback();
}

function renderDetail() {
  detailName.textContent = state.detail.name;
  const sells = state.detail.orders.filter((order) => order.side === 'sell');
  const buys = state.detail.orders.filter((order) => order.side === 'buy');
  minSell.textContent = sells.length ? `${Math.min(...sells.map((order) => order.platinum))}p` : '—';
  maxBuy.textContent = buys.length ? `${Math.max(...buys.map((order) => order.platinum))}p` : '—';
  orderCount.textContent = String(state.detail.orders.length);
  for (const [side, entries] of [['sell', sells], ['buy', buys]]) {
    const section = element('section', 'offer-section');
    const heading = element('div', 'offer-heading');
    const title = element('h2', '', side === 'sell' ? 'Sellers' : 'Buyers');
    title.id = `heading-${side}`;
    section.setAttribute('aria-labelledby', title.id);
    heading.append(title, element('span', 'offer-count', `${entries.length} shown`));
    section.append(heading);
    if (!entries.length) {
      section.append(element('p', 'no-offers', `No ${side} offers available.`));
    } else {
      const labels = element('div', 'order-labels');
      labels.setAttribute('aria-hidden', 'true');
      for (const label of ['Price', 'Trader', 'Status', 'Whisper']) labels.append(element('span', '', label));
      const list = element('ul');
      for (const order of entries) {
        const row = element('li', 'order-row');
        const trader = element('span', 'order-trader', order.trader);
        trader.title = order.trader;
        const presence = element('span', 'order-presence', {
          ingame: 'In game', online: 'Online', offline: 'Offline', unknown: 'Unknown',
        }[order.presence] ?? 'Unknown');
        presence.dataset.presence = order.presence;
        const button = element('button', 'copy-button', 'Copy whisper');
        button.type = 'button';
        button.dataset.order = order.id;
        button.dataset.side = order.side;
        button.setAttribute('aria-label', `Copy whisper to ${order.trader}`);
        copyButtons.push(button);
        row.append(element('span', 'order-price', `${order.platinum}p`), trader, presence, button);
        list.append(row);
      }
      section.append(labels, list);
    }
    orders.append(section);
  }
}

function renderCopyFeedback() {
  for (const button of copyButtons) {
    const active = copyFeedback?.id === button.dataset.order && copyFeedback?.side === button.dataset.side;
    const feedback = active ? copyFeedback.state : '';
    button.disabled = copying;
    button.dataset.state = feedback;
    button.setAttribute('aria-busy', String(feedback === 'pending'));
    button.textContent = { pending: 'Copying…', copied: '✓ Copied', failed: 'Retry copy' }[feedback] ?? 'Copy whisper';
  }
  copyStatus.dataset.state = copyFeedback?.state ?? '';
  copyStatus.textContent = {
    pending: 'Copying whisper…',
    copied: 'Whisper copied. Paste it into Warframe chat.',
    failed: 'Copy failed. Use Retry copy to try again.',
  }[copyFeedback?.state] ?? 'Copy whisper, then paste it into Warframe chat.';
}

function element(tag, className = '', text = '') {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}
