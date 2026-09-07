# Testing

Structural checks must finish in seconds:

```sh
tests/admission-store-smoke.sh
tests/ci-admission-smoke.sh
tests/catalog-stage-smoke.sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

These prove strict manifest/listing validation, inventory, native executable
rejection, optional browser-WASM admission, deterministic ZIP bytes, durable
receipt-last ingestion with exclusive admission, bounded reads that reject
special files, catalog search over 3840 structured items, and controller/query
state across view and controller restart. Warframe tests also cover reversed
response order, failed refresh with a valid cache, messages during startup,
and IndexedDB transaction aborts after a successful request. They do not prove
live compositor or game behavior.

The catalog-stage smoke removes the temporary build outputs after admission,
then proves that the CLI can produce a signed development catalog and the exact
content-addressed `.ocpkg` from the independently verified private store. Rust
tests verify the Ed25519 signature against the compiled development public key,
reject another seed, and cover the envelope, listing, manifest, URL, size, and
digest contract.

The catalog-site tests cover explicit native navigation links, multiple
versions per widget, exact-version duplicates, prerelease/build ordering, and
latest-version security statuses. Real browser-to-app opening still requires
an installed native build that registers the `overcrow` URI scheme; verify
startup, an existing hidden window, and first-run setup without installation
or activation from the link.

Also run the catalog-site contract:

```sh
node --test tests/site-runtime.test.js
```

These site tests include the historical production catalog display, refusal of
malformed legacy metadata and dependencies, removal of legacy native install
links, and parity between `web/marketplace/` and the staged production shell.
Run `node scripts/stage-marketplace-site.mjs` after UI edits and before the
site tests. No catalog signature is required for this website-only step.

The admission smoke uses real temporary Git revisions. It proves that an
undeclared file present only in the proposed revision is rejected, that a valid
proposal emits a receipt v2 bound to its revision, tree, identity, version, and
the SHA-256 and byte length of its package and listing, and that tests from an
exact trusted push cannot be silently skipped. Pull-request admission never
executes proposed JavaScript or build scripts; it validates and packages those
bytes with the tool compiled from the target-base commit. Push admission runs
the now-trusted revision's
Rust, Warframe Market, and site-runtime tests once. The Ubuntu CI runner installs
the distribution's Node package before these checks: the runner's preinstalled
Node lives under a writable directory and is intentionally rejected by the
system-Node resolver. The smoke is repeated only
on trusted pushes because pull requests cannot modify the CI trust boundary.
When a private accepted store is explicitly supplied, the smoke also proves
that the exact package and listing remain independently verifiable after the
driver's temporary artifact directory has been removed. Package or same-size
listing tampering, unreceipted files, same-version replacement, and downgrade
are covered by the Rust admission tests.

Production tests cover offline preparation, detached signature verification,
sequence reservations, interrupted finalization, and retained version statuses.
The public CLI smoke rejects untrusted signatures against the compiled production
key. Private signing remains external; the generic maintainer sandbox is still
pending. Catalog publication and OverCrow runtime reuse admitted bytes and never
rerun their tests.
