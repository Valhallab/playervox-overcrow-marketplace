# Security Policy

## Reporting a vulnerability

Do not open a public issue or publish a proof-of-concept exploit for an
unresolved vulnerability. Use
[GitHub Private Vulnerability Reporting](https://github.com/Valhallab/playervox-overcrow-marketplace/security/advisories/new)
to contact Valhallab SASU privately and identify this marketplace as the
affected project. Include the affected revision, impact, environment, and the
smallest safe reproduction. Never include user data, credentials, production
keys, or extension private storage.

Coordinated disclosure is appreciated. The project does not currently offer a
bug bounty or guarantee a response deadline.

## Submission and publication boundary

Community pull requests run in CI with split, minimal permissions. The
verification job has only `contents: read`; its automatic `GITHUB_TOKEN` is not
persisted by checkout. Two separate jobs have no checkout or content access and
receive only `statuses: write` to report the base-specific admission context on
the exact reviewed pull-request head. No repository or organization secret,
signing key, deployment credential, production sequence state, or other
publication authority is passed to a job step. Creators never receive signing
authority. Automation is evidence for a separate human review, and acceptance
into `candidate` does not publish a package. Production signing and promotion
remain offline maintainer operations against an exact reviewed revision.

The hosted `pull_request_target` job definition comes from the default branch,
not from the proposed revision. It fetches the exact pull-request head through
the fixed public repository URL as a Git object, verifies its expected digest,
and prepares its validator and drivers from the exact target-base commit. A
base-built planner compares every materialized regular file with the proposed
Git tree's mode, size, object ID, and portable path, so archive attributes
cannot omit or rewrite proposed bytes. The proposed tree is treated only as
data and is never selected as the Actions checkout or a shell-script source.

Admission validates listing metadata, the strict Web API v1 manifest, and its
complete file ledger with the base-built `marketplace-tool`, packages every
widget directory once, then inspects the stored-zip `.ocpkg`. A machine-readable
receipt binds the trust revision, proposed revision, proposed tree, source
directory, extension identity, version, package SHA-256, and byte length. No
proposed JavaScript, build command, test, Cargo manifest, or shell script is
executed by `pull_request_target`. Browser WebAssembly is allowed only as
verified page code. WIT, Wasmtime, native executable modules, and provider
graphs are rejected. Push CI executes the exact now-trusted revision's Rust and
JavaScript tests once. Later maintainer ingestion may execute untrusted build
or test code only in its dedicated sandbox; publication copies its admitted
bytes and does not rebuild or retest them.

The marketplace website cannot install software. The Control Center validates
packages and user consent; local unverified packages install disabled and stay
disabled until explicitly enabled.

If a listed package is suspected of compromise, maintainers may publish a
signed suspension or revocation in a newer monotonic catalog. Clients must
reject that package for new installation or update, immediately disable an
installed copy, and offer its removal. An absent or stale catalog never invents
a revocation. A catalog signature never bypasses package validation, user
consent, or runtime sandboxing. Follow the authoritative
[production operations runbook](docs/production-operations.md) for key
recovery, loss, compromise, suspension, revocation, and corrective rollback.
