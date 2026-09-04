# Contributing

Submit Web API v1 extensions only. A submission is a directory of web
files plus `manifest.json` and `listing.json`. Do not send WIT worlds,
Wasmtime components, native modules, or provider graphs.
Declared browser `.wasm` assets are permitted only as page-side computation
inside the normal Web sandbox; they do not receive native or system authority.

## Local checks

```sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Hosted pull-request CI packages the exact proposed Git tree with the
base-reviewed tool without executing proposed code. Push CI tests the exact
trusted revision. See `SECURITY.md` for the trust boundary and
`docs/testing.md` for the covered scenarios. Maintainer ingestion may later
build and test once in a sandbox, then reuse that exact artifact.

Do not commit production keys, `.ocpkg` outputs outside fixtures, or
changes under `published/` unless a later authorized publication task
asks for them.
