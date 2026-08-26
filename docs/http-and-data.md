# HTTP and host data

Components have no sockets. To read an API, declare the exact HTTPS host in the
manifest, obtain the user grant, and emit `http-get(request_id, host, path)`.
The host permits bounded `GET` requests only, validates DNS and the connected
peer, sends no ambient credentials, follows no redirects, and returns a typed
`http-result`. Request IDs are nonzero and strictly increasing. Handle a result
with no status as unavailable and never assume a body is UTF-8.

Storage is a bounded package-scoped key/value service. Use `storage-get`,
`storage-set`, or `storage-delete` only when storage was declared and granted;
results arrive as `storage-result`. Components never receive filesystem paths.

Providers publish one bounded payload with a strictly increasing revision and
a canonical `provider-id/schema.vN` ID. Dependent widgets receive only schemas
authorized by their exact installed dependency. Provider data is coalesced;
write handlers for the latest value, not for a message queue.

The optional `overcrow.session.v1` feed contains only the selected active game,
Steam app ID, elapsed milliseconds, overlay mode, and sanitized resource
telemetry. It contains no game memory, packets, paths, window titles, account
identifiers, or raw input. Capabilities are denied by default, and an update
that expands them requires new user consent.
