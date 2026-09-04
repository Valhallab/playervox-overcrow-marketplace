# Review policy

Reviewers admit one Web API v1 artifact.
Hosted pull-request admission first treats the exact proposed tree as data,
packages every widget with the base-reviewed tool, and emits an ephemeral
digest receipt. It does not execute proposed code. A maintainer sandbox is the
later boundary that may build and test proposed code once before producing the
durable reviewed artifact.

- Reject WIT, Wasmtime components, native executable modules, providers, and
  undeclared files. A declared browser `.wasm` asset is ordinary sandboxed page
  code and receives no native authority.
- Do not impose a suffix allowlist on declared regular Web data. The host maps
  known UI formats and serves unknown formats as non-sniffed opaque bytes;
  native suffixes and executable signatures remain rejected.
- Confirm the manifest file ledger matches the packaged bytes.
- Confirm listing locales, license, and source URL are exact and
  non-executable.
- If a `build.command` is declared, run it once in the maintainer
  sandbox, then package the output directory once.
- Sign catalog identity, version, digest, and size. Do not rebuild or
  retest after ingestion.

Publication remains a separate offline step. This document does not
authorize a push or deployment.
