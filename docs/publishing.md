# Publishing

This increment is local-only. It does not authorize deployment to
`overcrow.playervox.com`, public submissions, production signing, or a public
claim that arbitrary third-party code is safe.

A future publication must use one validated source record to generate both the
human site and machine catalog. Packages bind exact IDs, versions, digests, and
sizes; the catalog is canonical, monotonic, expiring, and signed only after
automated checks plus human approval.

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

No command in this repository performs production signing. Do not add a
private key, passphrase, key path, deployment credential, or publishing
endpoint to local configuration, generated output, CI logs, or a commit.

Public launch additionally requires the security gates in
[SECURITY.md](../SECURITY.md), a key-operations and recovery procedure, and an
incident suspension/revocation runbook. Deployment remains a separate,
explicitly authorized operation.
