# Production Marketplace Operations

This runbook is the sole operational source of truth for production
publication. It defines preparation and review of the public trust change; it
does not authorize a push, deployment, or publication.

## 1. Current product

OverCrow Marketplace admits Web API v1 extensions only: a web directory,
`manifest.json`, `listing.json`, and a deterministic stored-zip `.ocpkg`
from `marketplace-tool package`. WIT, Wasmtime, native widgets, and
provider graphs are retired.

`published/` is the tracked production site served by Coolify. Its
`marketplace/v1/` subtree contains the signed Web API v1 catalog with Warframe
Market. Historical native-era packages and previews remain at their original
URLs, but are not entries in the current catalog. Website-only updates preserve
that subtree byte-for-byte and do not rotate keys.

## 2. Preconditions and role separation

Use separate clean worktrees and roles: contributors submit candidate PRs;
hosted CI materializes the exact proposed tree and produces ephemeral package
digests without executing its code; a maintainer reviews and ingests the exact
trusted revision into a private store; acceptance merges only to `candidate`; an
offline publisher prepares exact catalog bytes and verifies a detached signature; and a separate deployment
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

## 5. Prepare and finalize production output offline

Production uses two independent commands. Neither command accepts a private key,
contacts a server, starts a signer, edits `published/`, or deploys output. The
production public key and key ID are compiled from
`keys/overcrow-production-2026-01.pub`; the origin is fixed to
`https://overcrow.playervox.com/marketplace/v1/`. Development keys and loopback
URLs cannot authorize finalization.

Keep four distinct private directories outside Git working trees: the accepted
store, publication state, prepared tree, and finalized output. The previous
finalized output is another separate directory. Each root must already exist,
be canonical, owned by the operator, and mode `0700`. Every ancestor must be
a real directory owned by root or the operator. Group- or world-writable
ancestors require the sticky bit; every child along that path must still be
owned by root or the operator. This permits protected temporary roots such as
`/tmp` and `/var/tmp`, but rejects replaceable paths under ordinary shared
directories. Symlink components are rejected. Output starts empty;
repeating the same operation may reuse only its exact matching files. Extra,
modified, or linked output files are rejected. Do not put another output or the
publication state inside one of these roots.

### Initial Web API v1 migration

The historical native-era snapshot is not an accepted Web API v1 previous
output. Before the first preparation, independently establish the highest
production sequence ever signed from the verified production catalog and the
offline publisher's records. Set `previousSequence` to that high-water mark,
including any later sequence reserved by an interrupted publication. Set
`sequence` to exactly that number plus one. Do not guess, default to zero, or
use an expired downloaded catalog as proof of the latest sequence. If the
high-water mark cannot be established, stop publication and recover the offline
records. Initial bootstrap is an explicit operator assertion; the tool cannot
infer prior production history from an empty state directory.

The first preparation omits `--previous-output`. It creates a Web API v1 output
from the selected accepted receipt. It does not import legacy manifests,
previews, or archives. The separate deployment operator must preserve historical
package URLs from the old snapshot during this migration; do not replace the
live package directory with only the new bootstrap tree or delete old objects.

### Preparation

Create a small request outside the checkout, for example:

```json
{
  "schemaVersion": 1,
  "sequence": 43,
  "previousSequence": 42,
  "generatedAt": "2026-09-05T12:00:00Z",
  "statuses": []
}
```

Those sequence numbers and time are illustrative. Use the independently
confirmed high-water mark and the current canonical UTC time
(`YYYY-MM-DDTHH:MM:SSZ`). Expiration is calculated as exactly 90 days after
`generatedAt`; future generation and already expired requests are rejected.

For the initial migration, run:

```sh
cargo run -p marketplace-tool --locked -- prepare-production-catalog \
  --store "$accepted_store" --review-tree "$review_tree" \
  --state "$publication_state" --request "$request_json" \
  --output "$prepared_output"
```

For every subsequent release, append
`--previous-output "$previous_finalized_output"`. It must contain a completed
production-signed Web API v1 catalog from this tool, with the fixed origin,
90-day lifetime and `preview: null`, plus its exact package inventory. An expired
previous catalog may be used as authenticated history, but a new preparation
must have a current validity window. Its envelope digest must match the last
signature recorded in publication state. The tool does not accept native-era
or arbitrary third-party catalog layouts through this option.

Preparation verifies the admitted receipt, listings and archives, then writes
`payload.json` and `packages/<id>/<version>/<sha256>.ocpkg`; it commits
`preparation.json` last. Review the exact payload and its SHA-256, including
sequence, times, identities, versions, permissions, licenses and statuses.
Do not reformat or edit prepared bytes.

New admitted versions receive `verified` status. Prior `(id, version)` entries
and packages are retained, including old versions no longer offered by the
current receipt. Same-version replacement, listing substitution and downgrade
are rejected. Use explicit status changes for withdrawal or security action:

```json
"statuses": [
  {"id": "com.example.widget", "version": "1.2.3", "status": "revoked"}
]
```

Allowed statuses are `verified`, `security-suspended` and `revoked`. Suspension
persists until an explicit change lifts it. Revocation is permanent for that
version; a later admitted version can be verified independently. Omission from
a request never removes an old status, version or archive. There is no automatic
withdrawal-by-absence or garbage collector.

### Detached signature and finalization

Transfer the exact `payload.json` bytes and their reviewed digest to the separate
authorized offline signing process. Request an Ed25519 signature over those raw
bytes, without prehashing or JSON normalization. Bring back only the raw 64-byte
signature file. The private key remains in that external signing process; do
not copy it into a checkout, prepared tree, tool argument, CI job or log.

```sh
cargo run -p marketplace-tool --locked -- finalize-production-catalog \
  --prepared "$prepared_output" --state "$publication_state" \
  --signature "$detached_signature" --output "$finalized_output"
```

Finalization checks the reservation, prepared marker, canonical payload, current
expiry, exact package bytes and detached signature against the compiled
production public key. It copies the archives, persists the verified envelope
digest in publication state, and commits `catalog.json` last. Output contains
only `catalog.json` and its complete content-addressed package tree. Successful
finalization prepares a deployable artifact; it does not deploy or grant approval
to deploy it. Review and authorize that external action separately.

### State, concurrency, and recovery

Publication state contains one private atomically replaced `state.json`: the
highest reserved sequence, exact payload digest, validity window, previous
catalog digest and, after signature verification, completed envelope digest.
It contains no private key or signature authority. Retain and back it up outside
repositories together with the last finalized output. Deleting or restoring an
older state file can discard anti-rollback knowledge; never do that as routine
recovery.

Both commands hold a nonblocking exclusive directory lock on state and output.
A concurrent operation fails immediately. Retry once the current operation has
finished; there is no stale lock file to delete. Exact replay is idempotent,
but another payload cannot reuse a sequence and an older preparation cannot be
finalized after the high-water mark advances.

After an interruption, repeat the same request and output to complete the exact
matching files. The admission bytes and previous finalized tree must remain
available when repeating preparation. If an output contains a damaged file,
use a new empty private output with the same state and request; do not edit the
state or sign modified prepared bytes. Finalization can resume after its
signature digest was recorded but before `catalog.json` became visible. The
next preparation requires that recovered signed output as its predecessor.
An unsigned reservation must be completed before another sequence is prepared;
once its 90-day validity has expired, a new request may reserve the next sequence
using the last completed predecessor. Expired signatures remain unusable.

The tool bounds a request to 128 KiB, payload to 700 KiB, envelope to 1 MiB,
catalog to 500 version entries, and each archive to 128 MiB. Inventory traversal
is limited to 2,004 entries and four levels. Archives are validated and copied
one at a time, never accumulated in memory. Retention can consume up to
62.5 GiB on disk per complete tree at the archive/count limits; allow space for
both prepared and finalized copies. Reaching a retention limit stops publication
and requires a separately reviewed retention policy, not silent deletion of
revocations or old package URLs.

## 6. Keys and authority material

Production private keys, monotonic state and recovery backups stay outside the
repository. The old WASM-era and generic source-bundle publishers remain
retired. The development stager remains restricted to its public fixture key
and loopback origin. Production preparation/finalization has a separate entry
point and never imports or reads private signing material.

## 7. Website-only publication

UI/UX changes are independent of catalog signing. After editing
`web/marketplace/`, stage and test the website shell:

```sh
node scripts/stage-marketplace-site.mjs
node --test tests/site-runtime.test.js
git diff --check
git diff --exit-code -- published/marketplace/v1/
git status --short
```

Review and commit the source, generated shell and branding assets together.
The stager copies the fixed production policy and assigns content-hashed names
to scripts and CSS, so a new deployment does not reuse an earlier CDN asset.
It never reads a private key or modifies `marketplace/v1/`. Tests compare the
published shell with its sources and render the existing public catalog.
Coolify must continue to serve `published/`; do not point it at `web/`, which
contains source templates and no signed catalog.

After the authorized push/deployment, fetch `/marketplace/` and its referenced
CSS/JavaScript and compare them with the committed output. A successful CI or
push alone is not evidence that the deployment serves the new UI. Do not purge
Cloudflare as a substitute for checking the deployed bytes.

The website's historical-catalog reader displays legacy widget listings as
unavailable in the current app, with no native install link. Provider-only
entries stay hidden and their permissions remain visible on dependent widget
details. The current Web API v1 catalog does not use this compatibility path;
the reader does not authorize native-era package admission or installation.
The signed catalog, historical package URLs and preview objects remain
unchanged during a website-only update.

This document does not authorize a push, key rotation, catalog signature, or
Coolify deploy.
