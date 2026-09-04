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
hosted CI materializes the exact proposed tree and produces ephemeral package
digests without executing its code; a maintainer reviews and ingests the exact
trusted revision into a private store; acceptance merges only to `candidate`; an
offline publisher would create a new signed catalog; and a separate deployment
operator configures Coolify to serve tracked output. The hosted receipt is
evidence for review, not a durable accepted artifact and not publication
authority. Coolify, GitHub, CI, and project temporary files never receive
production authority material.

The current ingestion path accepts only an exact trusted push and its committed
built web files. It does not execute extension-defined build commands. A future
generic maintainer sandbox is a separate milestone.

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

Run the fast repository checks during development:

```sh
tests/admission-store-smoke.sh
tests/ci-admission-smoke.sh
tests/catalog-stage-smoke.sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
node --test tests/site-runtime.test.js
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

For a reviewed, clean, already trusted revision, persist the packages produced
by the same test/package pass into a private store outside all repositories:

```sh
repository=/absolute/path/to/a/clean/marketplace-checkout
private_parent=/absolute/private/path/to/admission-work
accepted_store=/absolute/private/path/to/accepted-store
/usr/bin/install -d -m 0700 -- "$private_parent" "$accepted_store"
revision=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
test -z "$(/usr/bin/git -C "$repository" status --porcelain=v1 --untracked-files=all)"
(
  CDPATH='' cd -- "$repository"
  sh scripts/ci-verify.sh \
    "$repository" "$revision" "$revision" push \
    Valhallab/playervox-overcrow-marketplace candidate \
    Valhallab/playervox-overcrow-marketplace candidate \
    "$private_parent" admission "$accepted_store"
)
review_tree=$(/usr/bin/git -C "$repository" rev-parse --verify "$revision^{tree}")
cargo run --manifest-path "$repository/tools/marketplace-tool/Cargo.toml" \
  -p marketplace-tool --locked -- verify-admission \
  --store "$accepted_store" --review-tree "$review_tree"
```

The driver runs the trusted revision's Rust and JavaScript tests once, packages
each widget once, and passes those admission outputs directly to
`marketplace-tool ingest`. Ingestion re-inspects the `.ocpkg`; it never executes,
repackages, or retests it. The store contains only:

```text
accepted-store/
├── admissions/<review-tree>.tsv
├── listings/<extension-id>/<version>/<sha256>.json
└── packages/<extension-id>/<version>/<sha256>.ocpkg
```

Package, listing, and receipt files are committed with synchronized temporary
files and atomic no-replace hard links. A receipt is written only after every
referenced package and listing is durable, so an interrupted attempt may leave
harmless content-addressed bytes but never a completed admission. Exact replay
is idempotent. Ingestion holds an exclusive lock on the private store directory
through policy validation and receipt verification. A concurrent ingestion
fails immediately; retry after the current ingestion finishes. No lock file
needs removal after an interrupted process. A completed same-version package or listing with different bytes,
and any downgrade, are rejected. Recovery uses the last verified completed
receipt or a fresh private store; do not edit a receipt, listing, or package in
place.

Receipts expose Git object IDs, extension IDs, versions, package/listing sizes,
and digests. Packages and listings expose the reviewed public extension bytes.
The store contains no private key, token, user data, extension storage, catalog
sequence, or deploy credential, but its path and contents still remain private
operator data.

These commands prove that candidate bytes cannot be replaced by base bytes,
that trusted push tests execute, and that admitted bytes survive temporary-work
cleanup. If hosted admission cannot produce an exact-tree receipt, stop
accepting candidate changes; do not fall back to the base checkout. Nothing in
this section signs a catalog, touches `published/`, or deploys Coolify.

To exercise the complete local contract after admission, create a fresh private
serve tree and stage a development catalog. The command accepts only the
repository's intentionally public development fixture key and fixed loopback
origin; it cannot create a production catalog.

```sh
serve_root=/absolute/private/path/to/development-marketplace
development_output="$serve_root/marketplace/v1"
generated_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
expires_at=$(date -u -d '+30 days' +%Y-%m-%dT%H:%M:%SZ)
/usr/bin/install -d -m 0700 -- "$serve_root" "$development_output"
cargo run --manifest-path "$repository/tools/marketplace-tool/Cargo.toml" \
  -p marketplace-tool --locked -- stage-development-catalog \
  --store "$accepted_store" --review-tree "$review_tree" \
  --output "$development_output" --sequence 1 \
  --generated-at "$generated_at" --expires-at "$expires_at" \
  --signing-key "$repository/fixtures/keys/development-ed25519.key"
python3 -m http.server 8787 --bind 127.0.0.1 --directory "$serve_root"
```

Use current canonical UTC timestamps and a positive sequence for an actual
manual run. The expiry must follow generation by no more than 90 days. The
output directory must be empty, private, and owned by the caller. Staging first
re-verifies the completed receipt, listing, manifest, package size, and digest;
it copies package bytes without rebuilding and commits `catalog.json` last.
Starting the loopback server is a foreground local test action, not a deploy.

## 5. Keys and authority material

The reviewed public key remains `keys/overcrow-production-2026-01.pub`.
Production private keys, sequence counters, and recovery backups stay outside
this repository. The WASM-era publisher and generic source-bundle publisher
scripts are deleted. Do not reconstruct them. The local development stager is
hard-coded to the development key ID and loopback origin; do not convert it
into a production signer by changing those constants. A later authorized task
must introduce a separate production signing boundary that consumes only a
verified accepted store before any new production catalog is published.

## 6. Live snapshot

Until a new signed Web API v1 catalog is authorized, Coolify continues to
serve the existing `published/` tree. Do not delete, rewrite, or force-push
that snapshot as cleanup.

This document does not authorize a push, key rotation, catalog signature, or
Coolify deploy.
