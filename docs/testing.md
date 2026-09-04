# Testing

Structural checks must finish in seconds:

```sh
tests/admission-store-smoke.sh
tests/ci-admission-smoke.sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

These prove strict manifest/listing validation, inventory, native executable
rejection, optional browser-WASM admission, deterministic ZIP bytes, durable
receipt-last ingestion, catalog search over 3840 structured items, and
controller/query state across view and controller restart. They do not prove
live compositor or game behavior.

Also run the Web API v1 catalog-site contract:

```sh
node --test tests/site-runtime.test.js
```

The admission smoke uses real temporary Git revisions. It proves that an
undeclared file present only in the proposed revision is rejected, that a valid
proposal emits a receipt v2 bound to its revision, tree, identity, version, and
the SHA-256 and byte length of its package and listing, and that tests from an
exact trusted push cannot be silently skipped. Pull-request admission never
executes proposed JavaScript or build scripts; it validates and packages those
bytes with the tool compiled from the target-base commit. Push admission runs
the now-trusted revision's
Rust, Warframe Market, and site-runtime tests once. The smoke is repeated only
on trusted pushes because pull requests cannot modify the CI trust boundary.
When a private accepted store is explicitly supplied, the smoke also proves
that the exact package and listing remain independently verifiable after the
driver's temporary artifact directory has been removed. Package or same-size
listing tampering, unreceipted files, same-version replacement, and downgrade
are covered by the Rust admission tests.

The generic maintainer sandbox is still pending. Catalog generation and
OverCrow runtime must reuse admitted bytes and never rerun their tests.
