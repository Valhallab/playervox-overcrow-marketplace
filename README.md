# OverCrow Widget Marketplace

This independent repository contains the local-first marketplace, creator
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

The generated `/public` directory is ignored and reproducible. The build stages
component binaries privately and never retains them in source package
directories. Serve `/public` through a local loopback HTTP server to browse the
catalog. The static site uses English by default, offers French application
copy, loads no remote resources, and is labelled **Development - unverified**.
It displays catalog information only; installation happens in the Control
Center. Installed packages stay visible in the Available list with an Installed
marker and appear in the overlay's Widget library, where activation and
visibility are managed. A local unverified `.ocpkg` always requires explicit
confirmation.

Reviewed community submissions are accepted under
[`community/<publisher>/<widget-id>/`](community/README.md) through pull
requests to `candidate`. Hosted CI admits metadata and repository shape without
executing submitted code. A maintainer then runs the complete sandboxed build
for the affected widget and reviews its exact source, capabilities,
dependencies, license, and provenance. Unchanged accepted widgets keep their
reviewed component bytes and are not rebuilt for another submission. Merge
acceptance is not publication; a later maintainer-owned offline release
promotes the accepted review bundle without retesting it or giving creators
signing authority.

The public deployment contract uses <https://overcrow.playervox.com/> for the
landing page, <https://overcrow.playervox.com/marketplace/> for browsing, and
<https://overcrow.playervox.com/marketplace/v1/catalog.json> for the catalog.
Production catalogs are valid for exactly 90 days. Republish no later than
60 days after issuance, on every content change, and immediately for a signed
security suspension or revocation. Operational rollback always uses a higher
catalog sequence. A visual builder can produce the same reviewed package format
in a later milestone; the current creator path is the Rust SDK and source
review. The authoritative production release, recovery, GitHub protection,
deployment, cache, and incident procedure is
[docs/production-operations.md](docs/production-operations.md).

Local development catalogs are signed only with a clearly labelled development
fixture key and accepted only by debug OverCrow builds. Production
private keys and passphrases must never enter this repository, its artifacts,
logs, CI output, or temporary files.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[community submission contract](community/README.md), the
[review policy](docs/review-policy.md), and the
[security policy](SECURITY.md) before proposing content. Run:

```sh
scripts/check-policy.sh
```

PlayerVox marketplace tooling and official widget sources are licensed under
`AGPL-3.0-only`. Each contributed package must declare its own compatible SPDX
license and provenance.
