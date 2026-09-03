const MAX_ORDERS_PER_SIDE = 5;
const MAX_PLATINUM = 900_000;

export function parseOrders(payload) {
  const data = payload?.data;
  if (!data || typeof data !== 'object') {
    throw new Error('invalid orders');
  }
  const sell = parseSide(data.sell, 'sell');
  const buy = parseSide(data.buy, 'buy');
  return [...sell, ...buy];
}

export function whisperLine(order, item) {
  const intent = order.side === 'sell' ? 'WTB' : 'WTS';
  return `/w ${order.trader} Hi, ${intent} ${item} for ${order.platinum}p`;
}

function parseSide(rows, side) {
  if (!Array.isArray(rows)) {
    return [];
  }
  const accepted = [];
  for (const row of rows) {
    const order = parseOrder(row, side);
    if (!order) {
      continue;
    }
    accepted.push(order);
    if (accepted.length === MAX_ORDERS_PER_SIDE) {
      break;
    }
  }
  return accepted;
}

function parseOrder(row, expectedSide) {
  if (!row || typeof row !== 'object' || row.visible !== true) {
    return null;
  }
  if (row.type !== expectedSide) {
    return null;
  }
  const user = row.user;
  if (!user || user.platform !== 'pc') {
    return null;
  }
  const id = typeof row.id === 'string' ? row.id : '';
  if (!/^[A-Za-z0-9_-]{1,96}$/.test(id)) {
    return null;
  }
  const platinum = Number(row.platinum);
  if (!Number.isInteger(platinum) || platinum < 1 || platinum > MAX_PLATINUM) {
    return null;
  }
  const trader = sanitizeTrader(user.ingameName);
  if (!trader) {
    return null;
  }
  return {
    id,
    side: expectedSide,
    platinum,
    trader,
    presence: presence(user.status),
  };
}

function sanitizeTrader(value) {
  if (typeof value !== 'string') {
    return null;
  }
  const output = [...value]
    .filter((character) => character >= ' ' && character !== '/' && character !== '\\')
    .slice(0, 32)
    .join('')
    .trim();
  return output || null;
}

function presence(status) {
  if (status === 'ingame' || status === 'online' || status === 'offline') {
    return status;
  }
  return 'unknown';
}
