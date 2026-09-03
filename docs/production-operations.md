# Production Marketplace Operations

This runbook is the sole operational source of truth for production
publication. It defines preparation and review of the public trust change; it
does not authorize a push, deployment, or publication.

## 1. Current product

OverCrow Marketplace admits Web API v1 extensions only: a web directory,
`manifest.json`, `listing.json`, and a deterministic stored-zip `.ocpkg`
from `marketplace-tool package`. WIT, Wasmtime, native widgets, and
provider graphs are retired.

`published/` is the last production snapshot. It still contains the
historical native-era catalog bytes that Coolify currently serves. This
cleanup does not rewrite those bytes and does not rotate keys.

## 2. Preconditions and role separation

Use separate clean worktrees and roles: contributors submit candidate PRs;
hosted CI performs static admission; a maintainer reviews the exact revision;
acceptance merges only to `candidate`; an offline publisher would create a
new signed catalog; and a separate deployment operator configures Coolify to
serve tracked output. Coolify, GitHub, CI, and project temporary files never
receive production authority material.

The fixed production origin is
`https://overcrow.playervox.com/marketplace/v1/`. A production catalog is valid
for exactly 90 days: republish by day 60, on every content change, and
immediately for a signed security suspension or revocation. An older sequence
is never republished as a rollback.

## 3. Repository visibility and GitHub rulesets

The repository must remain public for public static hosting, but treat the
repository and every pull request as untrusted publication inputs. Verify that
invariant through the GitHub interface; stop if it cannot be established.
Keep the existing technical and human-review rulesets on `candidate` and
`master`. `candidate` must not change `published/`.

## 4. Local admission (no publication)

```sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
node --test tests/site-runtime.test.js
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Those commands prove packaging and listing. They do not sign a catalog,
touch `published/`, or deploy Coolify.

## 5. Keys and authority material

The reviewed public key remains `keys/overcrow-production-2026-01.pub`.
Production private keys, sequence counters, and recovery backups stay
outside this repository. The WASM-era publisher
(`prepare-marketplace-tool.sh`, `marketplace-tool build|bind-build|verify*`)
is deleted. Do not reconstruct it. A later authorized task must introduce a
Web API v1 signer before any new production catalog is published.

## 6. Live snapshot

Until a new signed Web API v1 catalog is authorized, Coolify continues to
serve the existing `published/` tree. Do not delete, rewrite, or force-push
that snapshot as cleanup.

This document does not authorize a push, key rotation, catalog signature, or
Coolify deploy.
