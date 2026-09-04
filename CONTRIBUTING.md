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

Hosted pull-request CI fetches the exact proposed Git object, materializes it
against the base-reviewed tree plan, and packages every widget once with the
base-reviewed tool. Proposed JavaScript remains data in `pull_request_target`;
it is not executed with that workflow's authority. The resulting receipt binds
each package digest and size to the proposed revision and tree. Push CI runs
the trusted revision's Rust and JavaScript tests once. Maintainer ingestion may
later run a declared `build.command` and widget tests once in a sandbox, then
reuse that exact artifact.

Do not commit production keys, `.ocpkg` outputs outside fixtures, or
changes under `published/` unless a later authorized publication task
asks for them.
