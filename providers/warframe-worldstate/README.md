# Warframe worldstate provider

This provider is the only marketplace component that contacts the public
Warframe world-state endpoint. It requests exactly
`https://api.warframe.com/cdn/worldState.php`, projects the response into a
bounded schema, and publishes
`com.playervox.overcrow.warframe.worldstate/worldstate.v1` for dependent
widgets. It has no storage, game-data, clipboard, socket, or filesystem access.

The JSON payload contains host-owned `capturedAtSecs`, status rows, fissures,
the active Sortie and Archon Hunt, and invasions. Provider object IDs are kept
when they are canonical. Missing invasion IDs are derived from bounded raw
identity fields before display labels are applied. Display fields contain
embedded English Warframe names with a bounded readable fallback; widgets
translate their own interface and state labels.

Responses are limited to 2 MiB, JSON depth 32, strings 512 bytes, and explicit
collection caps. Published JSON is limited to 256 KiB. Refreshes occur no more
than once per minute. Data is stale after five minutes: the provider reports
unavailability once, continues bounded retries, and publishes the next local
revision after a valid response. Hosts must keep delivering ticks after a
recoverable guest error; consumers must reject a payload whose
`capturedAtSecs` is outside their freshness window.

See [`data/README.md`](data/README.md) for label provenance. Tests use local
fixtures only and never contact the public API.
