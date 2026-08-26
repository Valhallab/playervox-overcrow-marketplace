# Widget Permissions

OverCrow extensions do not receive direct filesystem, process, D-Bus,
compositor, input-device, game-memory, or network access. They request narrow
host capabilities, and OverCrow renders their bounded view tree itself.

Permission review uses plain language and includes the union requested by a
widget and its exact dependencies:

- **HTTPS data:** exact lowercase hosts, brokered by OverCrow with bounded GET
  requests; the component has no socket access.
- **Game data:** a versioned, reviewed, read-only schema containing only fields
  explicitly supplied by OverCrow. Game memory, files, packets, and raw input
  are never exposed.
- **Private settings:** an extension-scoped bounded store, only when a future
  reviewed capability grants it.
- **Semantic actions:** stable element IDs and sanitized events such as
  `clicked`, `value-changed`, or `submitted`; no raw keyboard or mouse events.

Passive widgets receive no interaction events. New or expanded capabilities
require fresh review and user consent. Marketplace approval alone cannot add a
game-data schema or widen a host capability.
