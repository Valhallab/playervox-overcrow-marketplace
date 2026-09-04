# OverCrow Widget Marketplace

This repository admits, packages, and publishes OverCrow Web API v1
extensions. Application and overlay code remain in the OverCrow
repository.

Extensions are local web apps: HTML, CSS, JavaScript or TypeScript, and
any web framework. WASM is optional compute, never a required UI engine.
OverCrow owns the outer chrome. The plugin owns its internal UX.

## Current status

The component/WIT/WASM product surface has been replaced with Web API
v1:

- `tools/marketplace-tool` validates the strict Web manifest, listing metadata,
  complete file ledger, and built bytes, then writes a deterministic stored-zip
  `.ocpkg`. Its admission commands bind each package to the validated
  `listing.json`, then persist and later re-verify both exact byte sequences in
  a private content-addressed store. Its development-only staging command signs
  a catalog and copies packages directly from one verified admission.
- `fixtures/hello-web` is the structural Web API v1 fixture.
- `widgets/warframe-market` is the reference extension: persistent
  controller, IndexedDB catalog and last query (~3840 structured items),
  `overcrow.fetch` to `api.warframe.market`, and a view that can
  hide/show without resetting search state.
- `published/` remains the historical production snapshot and is not
  rewritten by this reset.

Package locally:

```sh
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Pull-request admission is static and ephemeral. Trusted-push admission runs the
repository tests once, packages each widget once, and can ingest those exact
package and listing bytes into an explicitly supplied private store. Ingestion
re-inspects but does not execute, rebuild, or retest a package. Development
catalog staging reuses the stored bytes exactly. Runtime OverCrow verifies
signature, digest, and capabilities; it does not rebuild or retest widgets.
The generic maintainer sandbox for extension-defined build commands and the
production catalog signer are not implemented yet.

Declared browser `.wasm` assets are allowed for sandboxed page-side
computation. WIT/Wasmtime components and native executable modules remain
unsupported.

Production catalogs remain at
<https://overcrow.playervox.com/marketplace/v1/catalog.json> with a
90-day lifetime. Do not push, publish, or deploy from this reset.
Production private keys never enter this repository.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[review policy](docs/review-policy.md), and
[SECURITY.md](SECURITY.md) before proposing content.

PlayerVox marketplace tooling is licensed under `AGPL-3.0-only`. Each
package must declare its own compatible SPDX license.
