# Review policy

Reviewers admit one Web API v1 artifact.

- Reject WIT, Wasmtime, native modules, providers, and undeclared files.
- Confirm the manifest file ledger matches the packaged bytes.
- Confirm listing locales, license, and source URL are exact and
  non-executable.
- If a `build.command` is declared, run it once in the maintainer
  sandbox, then package the output directory once.
- Sign catalog identity, version, digest, and size. Do not rebuild or
  retest after ingestion.

Publication remains a separate offline step. This document does not
authorize a push or deployment.
