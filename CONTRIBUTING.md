# Contributing

Submit Web API v1 extensions only. A submission is a directory of web
files plus `manifest.json` and `listing.json`. Do not send WIT worlds,
Wasmtime components, native modules, or provider graphs.

## Local checks

```sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Hosted CI runs those structural checks. Maintainer admission may run a
declared `build.command` once in a sandbox, then reuses the exact
artifact.

Do not commit production keys, `.ocpkg` outputs outside fixtures, or
changes under `published/` unless a later authorized publication task
asks for them.
