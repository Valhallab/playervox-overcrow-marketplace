# Review policy

Reviewers admit one Web API v1 artifact.
Hosted pull-request admission first treats the exact proposed tree as data,
packages every widget with the base-reviewed tool, and emits an ephemeral
digest receipt. It does not execute proposed code. A maintainer sandbox is the
future boundary that may build and test proposed code once. Today, a trusted
push runs the repository checks and packages each committed widget once; an
operator may explicitly ingest those exact packages into a private store.

- Reject WIT, Wasmtime components, native executable modules, providers, and
  undeclared files. A declared browser `.wasm` asset is ordinary sandboxed page
  code and receives no native authority.
- Do not impose a suffix allowlist on declared regular Web data. The host maps
  known UI formats and serves unknown formats as non-sniffed opaque bytes;
  native suffixes and executable signatures remain rejected.
- Confirm the manifest file ledger matches the packaged bytes.
- Confirm listing locales, declared license, and source URL are exact and
  non-executable. Check that required license notices are packaged. Metadata
  validity does not approve a license for publication. PlayerVox widgets use
  MIT; the policy for third-party creators' widgets remains undecided. Do not
  infer a universal MIT requirement from the marketplace's own license; see
  [licensing scope](../LICENSING.md).
- Until the generic maintainer sandbox exists, require built web files in the
  reviewed tree; do not execute an extension-defined `build.command`.
- At ingestion, re-inspect every package and listing and require identity,
  version, both SHA-256 values, both sizes, and the exact-revision receipt to
  agree. Write the receipt last.
- Permit exact idempotent replay, but reject same-version replacement and
  downgrade relative to completed admissions.
- Sign catalog identity, version, digest, and size. Do not rebuild or
  retest after ingestion.

The development stager proves that last step locally from a completed
admission, using only the public fixture key and loopback origin. Production
signing remains a separate offline boundary.

Publication remains a separate offline step. This document does not
authorize a push or deployment.
