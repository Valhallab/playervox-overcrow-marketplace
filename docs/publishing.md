# Publishing

Publication consumes a completed admission from the private accepted store. It
never compiles, retests or mutates widget bytes.

`marketplace-tool prepare-production-catalog` creates an exact unsigned payload
and copies admitted and retained archives into a private output outside the
checkout. It reserves the next monotonic sequence in separate private operator
state. A separate authorized offline signer returns a raw Ed25519 signature;
`marketplace-tool finalize-production-catalog` verifies it against the compiled
production public key, rechecks every byte and commits `catalog.json` last.
Neither command receives a private key, contacts a server, edits `published/`,
or deploys a site.

Production retains previous `(extension ID, version)` targets and package URLs.
Use explicit `security-suspended` or permanent `revoked` status changes;
omission never erases a security decision. Catalog expiry is exactly 90 days.
Exact replay and interrupted operations reuse the reserved payload; conflicting
or stale sequences fail closed.

Read [production operations](production-operations.md) for the exact commands,
external state backup, initial legacy-snapshot sequence bootstrap, retention
limits and recovery rules. The historical native-era snapshot requires the
explicit migration procedure and is never silently imported or overwritten.

`marketplace-tool stage-development-catalog` remains a separate local test path
restricted to the public development fixture key and fixed loopback origin.
This document does not authorize signing, pushing, or deployment.
