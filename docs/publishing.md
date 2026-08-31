# Publishing

Community intake is open through pull requests to `candidate`, but merge
acceptance is not publication and makes no security certification. Read-only CI
provides automated evidence, human maintainers review the exact revision, and a
later repository-local `release/*` pull request to `master` may carry output
from the offline publisher. Creators receive no signing or deployment
credentials.

This repository supplies the read-only check and CODEOWNERS declarations, but
GitHub branch protection, the required check, and required CODEOWNER review are
separate operational configuration. Until those controls are configured, CI
output is evidence rather than stand-alone enforcement against a pull request
that changes the workflow itself.

One validated source record generates both the human site and machine catalog.
Packages bind exact IDs, versions, digests, and sizes; the catalog is canonical,
monotonic, expiring, and signed only after automated checks plus human approval.

The development fixture key is visibly non-production and may be selected only
by the fixed debug trust path. Production signing must require an explicit
absolute key path. Tooling must never generate, copy, cache, print, or commit a
production private key or passphrase.

The local generation flow is:

```sh
scripts/build-local.sh
cargo run -p marketplace-tool --locked -- verify public/marketplace/v1/catalog.json
```

The script stages the provider first, refreshes its exact digest in the four
dependent manifests, builds the full signed development catalog, and copies
the static site into ignored `/public`. It uses a fixed development timestamp
and sequence state, so a rerun with unchanged inputs reproduces the same
objects. A changed payload requires a strictly higher development sequence;
never reset or reuse one. Source package directories never retain
`component.wasm` after publication.

The reviewed offline publisher is `scripts/build-production.sh`. It remains
disabled until the production ceremony commits the reviewed public key at
`keys/overcrow-production-2026-01.pub`; no private authority material belongs
in this repository. It accepts only an exact clean `release/*` commit and
external private files with the required ownership and modes, stages and
verifies the complete tree, advances the sequence, and atomically replaces
`published/`. This is a local build operation and does not deploy or push. The
first catalog expires exactly 30 days after issuance and maintainers republish
at least every 14 days.

Production verification also requires Bubblewrap, a delegated user systemd
scope, and a canonical regular Node executable selected from `PATH`. Node and
every directory in its resolved absolute path must be root-owned and not
group- or world-writable; the executable must be single-link. A user-managed
version-manager shim is intentionally rejected. The Node checks run without
network or a process view and under fixed CPU, task, virtual-address, resident
memory, swap, file, and wall-time limits. Release and CI hosts must provide
that system Node installation or production verification fails closed.

The deployment contract serves tracked production output at
<https://overcrow.playervox.com/>, with the marketplace at
<https://overcrow.playervox.com/marketplace/> and the signed catalog at
<https://overcrow.playervox.com/marketplace/v1/catalog.json>. The website
cannot install packages; installation remains a Control Center operation.

Do not add a private key, passphrase, private key path, deployment credential,
or publishing endpoint to local configuration, generated output, CI logs, or
a commit.

Production operation additionally requires the security gates in
[SECURITY.md](../SECURITY.md), a key-operations and recovery procedure, and an
incident suspension/revocation runbook. Deployment remains a separate,
explicitly authorized operation.
