# OverCrow Widget Marketplace

This independent repository will contain the local-first marketplace, creator
SDK, documentation, build tooling, and external widget sources for PlayerVox
OverCrow. Application and overlay code remain in the OverCrow repository.

## Current status

This repository currently defines contributor and review policy only. It does
not publish a catalog, accept arbitrary public submissions, provide production
signing material, or make any security certification. Those steps remain gated
on a separate sandbox security audit and explicit maintainer authorization.

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
