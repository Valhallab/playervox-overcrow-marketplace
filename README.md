# OverCrow Widget Marketplace

This independent repository will contain the local-first marketplace, creator
SDK, documentation, build tooling, and external widget sources for PlayerVox
OverCrow. Application and overlay code remain in the OverCrow repository.

## Current status

This repository provides the local Rust SDK, deterministic package and signed
catalog tooling, a static local catalog site, and five external Warframe
packages: Status, Void Fissures, Sortie & Archon, Invasions, and Market. The
first four use one hidden Warframe Worldstate Provider; Market is a standalone
bundle. The site makes this dependency visible on each applicable card without
showing the provider as a normal browseable package.

Generate the WebAssembly components, local catalog, and site with the bounded
local build:

```sh
scripts/build-local.sh
cargo run -p marketplace-tool --locked -- verify public/marketplace/v1/catalog.json
```

The generated `/public` directory is ignored and reproducible. The build
temporarily stages each `component.wasm` beside its package metadata and
removes it before exiting; source package directories must never retain or
commit those binaries. Serve `/public` through a local loopback HTTP server to
browse the catalog. The static site uses English by default and has a French
language switch. It uses no remote resources and is labelled
**Development - unverified**.

This repository does not deploy a public catalog, accept arbitrary public
submissions, provide production signing material, or make any security
certification. Those steps remain gated on human review, application
integration, and explicit maintainer authorization.

Local development catalogs are signed only with a clearly labelled development
fixture key and accepted only by debug OverCrow builds. Production
private keys and passphrases must never enter this repository, its artifacts,
logs, CI output, or temporary files.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[review policy](docs/review-policy.md), and the
[security policy](SECURITY.md) before proposing content. Run:

```sh
scripts/check-policy.sh
```

PlayerVox marketplace tooling and official widget sources are licensed under
`AGPL-3.0-only`. Each contributed package must declare its own compatible SPDX
license and provenance.
