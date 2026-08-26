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

Public launch additionally requires the security gates in
[SECURITY.md](../SECURITY.md), a key-operations and recovery procedure, and an
incident suspension/revocation runbook. Deployment remains a separate,
explicitly authorized operation.
