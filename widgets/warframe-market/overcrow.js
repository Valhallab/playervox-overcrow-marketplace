const MAX_JSON_BYTES = 1024 * 1024;
const MAX_JSON_DEPTH = 64;
const MAX_REQUEST_BODY_BYTES = 2 * 1024 * 1024;
const EMPTY_BODY = new ArrayBuffer(0);
const encoder = new TextEncoder();
const decoder = new TextDecoder();

export class OvercrowError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'OvercrowError';
    this.code = code;
  }
}

class OvercrowResponse {
  #body;
  #bodyUsed = false;

  constructor(metadata, body) {
    if (!Number.isInteger(metadata.status) || metadata.status < 100 || metadata.status > 599) {
      throw new OvercrowError('invalid_response', 'Native response status is invalid');
    }
    this.status = metadata.status;
    this.ok = metadata.status >= 200 && metadata.status < 300;
    this.contentType = typeof metadata.contentType === 'string' ? metadata.contentType : null;
    this.#body = copyArrayBuffer(body);
    Object.freeze(this);
  }

  get bodyUsed() {
    return this.#bodyUsed;
  }

  #consume() {
    if (this.#bodyUsed) {
      throw new OvercrowError('body_used', 'Response body has already been consumed');
    }
    this.#bodyUsed = true;
    return this.#body.slice(0);
  }

  async arrayBuffer() {
    return this.#consume();
  }

  async text() {
    return decoder.decode(this.#consume());
  }

  async json() {
    return JSON.parse(await this.text());
  }
}

function bridge() {
  const candidate = globalThis.__overcrowNative;
  if (!candidate
      || (candidate.role !== 'controller' && candidate.role !== 'view')
      || typeof candidate.request !== 'function'
      || typeof candidate.subscribe !== 'function') {
    return null;
  }
  return candidate;
}

function copyArrayBuffer(value) {
  if (!(value instanceof ArrayBuffer)) {
    throw new OvercrowError('invalid_response', 'Native response body is not an ArrayBuffer');
  }
  return value.slice(0);
}

function requestBody(value) {
  let body;
  if (value === undefined || value === null) {
    body = EMPTY_BODY.slice(0);
  } else if (value instanceof ArrayBuffer) {
    body = value.slice(0);
  } else if (ArrayBuffer.isView(value)) {
    body = value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
  } else if (typeof value === 'string') {
    body = encoder.encode(value).buffer;
  } else {
    throw new OvercrowError('invalid_body', 'Body must be a string, ArrayBuffer, or typed array');
  }
  if (body.byteLength > MAX_REQUEST_BODY_BYTES) {
    throw new OvercrowError('request_body_limit', 'Request body exceeds the 2 MiB limit');
  }
  return body;
}

function cloneJson(value, seen = new Set(), depth = 0) {
  if (depth > MAX_JSON_DEPTH) {
    throw new OvercrowError('invalid_message', 'Payload nesting exceeds the JSON depth limit');
  }
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value !== 'object') {
    throw new OvercrowError('invalid_message', 'Payload must contain cloneable JSON values only');
  }
  if (seen.has(value)) {
    throw new OvercrowError('invalid_message', 'Payload must not contain cycles');
  }
  const prototype = Object.getPrototypeOf(value);
  if (!Array.isArray(value) && prototype !== Object.prototype && prototype !== null) {
    throw new OvercrowError('invalid_message', 'Payload must contain plain JSON objects only');
  }
  seen.add(value);
  const cloned = Array.isArray(value) ? [] : {};
  for (const key of Object.keys(value)) {
    Object.defineProperty(cloned, key, {
      value: cloneJson(value[key], seen, depth + 1),
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  seen.delete(value);
  return cloned;
}

function checkedJson(value) {
  const cloned = cloneJson(value);
  if (encoder.encode(JSON.stringify(cloned)).byteLength > MAX_JSON_BYTES) {
    throw new OvercrowError('message_too_large', 'Payload exceeds the 1 MiB JSON limit');
  }
  return cloned;
}

async function invoke(metadata, body = EMPTY_BODY) {
  const native = bridge();
  if (!native) {
    throw new OvercrowError('bridge_unavailable', 'OverCrow native bridge is unavailable');
  }
  const checkedMetadata = checkedJson(metadata);
  let response;
  try {
    response = await native.request(checkedMetadata, copyArrayBuffer(body));
  } catch (_) {
    throw new OvercrowError('native_failure', 'OverCrow native request failed');
  }
  if (!response || typeof response !== 'object') {
    throw new OvercrowError('invalid_response', 'Native response is malformed');
  }
  const responseMetadata = checkedJson(response.metadata);
  const responseBody = copyArrayBuffer(response.body);
  if (!responseMetadata || typeof responseMetadata !== 'object' || Array.isArray(responseMetadata)) {
    throw new OvercrowError('invalid_response', 'Native response metadata is malformed');
  }
  if (responseMetadata.ok !== true) {
    const error = responseMetadata.error;
    if (!error || typeof error !== 'object' || Array.isArray(error)
        || typeof error.code !== 'string' || typeof error.message !== 'string') {
      throw new OvercrowError('invalid_response', 'Native rejection is malformed');
    }
    throw new OvercrowError(error.code, error.message);
  }
  return { metadata: responseMetadata, body: responseBody };
}

const nativeAtLoad = bridge();
const role = nativeAtLoad?.role ?? 'unavailable';
const relayListeners = new Set();
const visibilityListeners = new Set();
const gameListeners = new Map();

if (nativeAtLoad) {
  nativeAtLoad.subscribe((rawEvent) => {
    let event;
    try {
      event = checkedJson(rawEvent);
    } catch (_) {
      return;
    }
    if (!event || typeof event !== 'object' || Array.isArray(event)) return;
    if (event.type === 'relay'
        && event.source !== role
        && (event.source === 'controller' || event.source === 'view')) {
      for (const listener of [...relayListeners]) listener(event.payload);
    } else if (event.type === 'visibility' && typeof event.visible === 'boolean') {
      for (const listener of [...visibilityListeners]) listener(event.visible);
    } else if (event.type === 'gameEvent' && typeof event.event === 'string') {
      for (const listener of [...(gameListeners.get(event.event) ?? [])]) listener(event.payload);
    }
  });
}

function listen(collection, listener) {
  if (typeof listener !== 'function') {
    throw new OvercrowError('invalid_listener', 'Listener must be a function');
  }
  collection.add(listener);
  let active = true;
  return () => {
    if (active) collection.delete(listener);
    active = false;
  };
}

export const overcrow = Object.freeze({
  async fetch(url, options = {}) {
    if (typeof url !== 'string' || !options || typeof options !== 'object') {
      throw new OvercrowError('invalid_request', 'Fetch URL or options are invalid');
    }
    const method = options.method === undefined ? 'GET' : String(options.method).toUpperCase();
    if (!['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(method)) {
      throw new OvercrowError('invalid_request', 'Fetch method is not supported');
    }
    const response = await invoke(
      { type: 'fetch', method, url },
      requestBody(options.body),
    );
    return new OvercrowResponse(response.metadata, response.body);
  },
  game: Object.freeze({
    async snapshot() {
      const response = await invoke({ type: 'gameSnapshot' });
      return checkedJson(response.metadata.value);
    },
    on(event, listener) {
      if (typeof event !== 'string' || event.length === 0) {
        throw new OvercrowError('invalid_event', 'Event ID must be a non-empty string');
      }
      let listeners = gameListeners.get(event);
      if (!listeners) {
        listeners = new Set();
        gameListeners.set(event, listeners);
      }
      const unsubscribe = listen(listeners, listener);
      return () => {
        unsubscribe();
        if (listeners.size === 0) gameListeners.delete(event);
      };
    },
  }),
  runtime: Object.freeze({
    role,
    async send(payload) {
      const checkedPayload = checkedJson(payload);
      const target = role === 'controller' ? 'view' : 'controller';
      await invoke({ type: 'relay', target, payload: checkedPayload });
    },
    onMessage(listener) {
      return listen(relayListeners, listener);
    },
  }),
  lifecycle: Object.freeze({
    onVisibility(listener) {
      return listen(visibilityListeners, listener);
    },
  }),
  surface: Object.freeze({
    async invalidate() {
      await invoke({ type: 'invalidate' });
    },
  }),
  clipboard: Object.freeze({
    async writeText(text) {
      if (typeof text !== 'string') {
        throw new OvercrowError('invalid_request', 'Clipboard text must be a string');
      }
      await invoke({ type: 'clipboardWrite', text });
    },
  }),
});

try {
  Object.defineProperty(globalThis, 'overcrow', {
    value: overcrow,
    configurable: false,
    enumerable: false,
    writable: false,
  });
} catch (_) {
  // Module consumers still receive the named export when a page locked the global first.
}

export default overcrow;
