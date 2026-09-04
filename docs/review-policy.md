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
- Confirm listing locales, license, and source URL are exact and
  non-executable.
- Until the generic maintainer sandbox exists, require built web files in the
  reviewed tree; do not execute an extension-defined `build.command`.
- At ingestion, re-inspect every package and listing and require identity,
  version, both SHA-256 values, both sizes, and the exact-revision receipt to
  agree. Write the receipt last.
- Permit exact idempotent replay, but reject same-version replacement and
  downgrade relative to completed admissions.
- Sign catalog identity, version, digest, and size. Do not rebuild or
  retest after ingestion.

Publication remains a separate offline step. This document does not
authorize a push or deployment.
