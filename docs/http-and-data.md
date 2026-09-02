# HTTP and host data

Components have no sockets. To read an API, declare the exact HTTPS host in the
manifest, obtain the user grant, and emit `http-get(request_id, host, path)`.
The host permits bounded `GET` requests only, validates DNS and the connected
peer, sends no ambient credentials, follows no redirects, and returns a typed
`http-result`. Request IDs are nonzero and strictly increasing. Handle a result
with no status as unavailable and never assume a body is UTF-8.

API v1 delivers one bounded response body. API v2 keeps the same 2 MiB request
limit but delivers a start event, at most 32 chunks of at most 64 KiB, and an
end event so a component can parse incrementally. Warframe Market uses this
path for the item catalog: it streams the initial response into a compact,
content-checked index split across bounded private-storage entries. A fresh
index is reused for 24 hours, stale data remains searchable while one refresh
runs, and the manifest is written last so an interrupted refresh cannot replace
the previous cache. Per-item order responses remain separately bounded and are
never stored in that catalog cache.

Storage is a bounded package-scoped key/value service. Use `storage-get`,
`storage-set`, or `storage-delete` only when storage was declared and granted;
results arrive as `storage-result`. Components never receive filesystem paths.

Providers publish one bounded payload with a revision that strictly increases
within the current runner generation and a canonical
`provider-id/schema.vN` ID. The host maps this local revision onto a globally
increasing broker revision and rejects output from stopped generations.
Dependent widgets receive only schemas authorized by their exact installed
dependency. Provider data is coalesced; write handlers for the latest value,
not for a message queue.

Provider errors are recoverable lifecycle signals. The host keeps delivering
scheduled ticks after a guest reports temporary unavailability so a provider
can retry within its declared cadence. A dependent widget must still validate
the provider-owned capture timestamp and render stale data as unavailable;
retaining a previous broker value does not make it fresh.

The optional `overcrow.session.v1` feed contains only the selected active game,
Steam app ID, elapsed milliseconds, overlay mode, and sanitized resource
telemetry. It contains no game memory, packets, paths, window titles, account
identifiers, or raw input. Capabilities are denied by default, and an update
that expands them requires new user consent.
