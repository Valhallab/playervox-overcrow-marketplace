# Testing

Structural checks must finish in seconds:

```sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

These prove strict manifest/listing validation, inventory, native executable
rejection, optional browser-WASM admission, deterministic ZIP bytes, catalog
search over 3840 structured items, and controller/query state across view and
controller restart. They do not prove live compositor or game behavior.

Also run the Web API v1 catalog-site contract:

```sh
node --test tests/site-runtime.test.js
```

Maintainer admission may run widget tests once before packaging. Catalog
generation and OverCrow runtime reuse the admitted bytes and never rerun
those tests.
