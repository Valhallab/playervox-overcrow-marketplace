# OverCrow Widget Marketplace

This independent repository will contain the local-first marketplace, creator
SDK, documentation, build tooling, and external widget sources for PlayerVox
OverCrow. Application and overlay code remain in the OverCrow repository.

## Current status

This repository currently provides the local Rust SDK, deterministic package
and signed-catalog tooling, a hello proof of concept, and the external
Warframe worldstate provider with Status, Void Fissures, Sortie and Archon
Hunt, and Invasions widget sources. The Warframe packages are not yet listed
in the development catalog. This repository does not deploy a public catalog,
accept arbitrary public submissions, provide production signing material, or
make any security certification. Those steps remain gated on human review,
application integration, and explicit maintainer authorization.

Future local development catalogs will be signed only with a clearly labelled
development fixture key and accepted only by debug OverCrow builds. Production
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
