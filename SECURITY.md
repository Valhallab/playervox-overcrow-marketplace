# Security Policy

## Reporting a vulnerability

Do not open a public issue or publish a proof-of-concept exploit for an
unresolved vulnerability. Use
[GitHub Private Vulnerability Reporting](https://github.com/Valhallab/playervox-overcrow-marketplace/security/advisories/new)
to contact Valhallab SASU privately and identify this marketplace as the
affected project. Include the affected revision, impact, environment, and the
smallest safe reproduction. Never include user data, credentials, production
keys, or provider payloads.

Coordinated disclosure is appreciated. The project does not currently offer a
bug bounty or guarantee a response deadline.

## Submission and publication boundary

Community pull requests run in CI with read-only repository permissions and no
secret, signing key, deployment credential, production sequence state, or
other publication authority. Creators never receive signing authority.
Automation is evidence for a separate human review, and acceptance into
`candidate` does not publish a package. Production signing and promotion remain
offline maintainer operations against an exact reviewed revision.

The hosted `pull_request_target` job definition comes from the default branch,
not from the proposed revision. It fetches the exact pull-request head through
the fixed public repository URL as a Git object, verifies its expected digest,
and prepares its validator and drivers from the exact target-base commit. The
proposed tree is treated only as data and is never selected as the Actions
checkout or a shell-script source. Admission performs bounded metadata, path,
manifest, and private-material checks, then exits before staging, compilation,
tests, or any other candidate execution. A proposed root Cargo configuration
fails admission; it is never used to prepare the validator.

The maintainer's complete gate uses the same trusted-base boundary. Candidate
format, lint, native tests, component builds, and web tests then run without
network, secrets, a host process view, a writable source checkout, or writable
later evidence. Unsupported sandbox primitives fail closed; they are never
replaced by an unsandboxed build.

The marketplace website cannot install software. The Control Center validates
packages and user consent; local unverified packages install disabled and stay
disabled until explicitly enabled.

If a listed package is suspected of compromise, maintainers may publish a
signed suspension or revocation in a newer monotonic catalog. Clients must
reject that package for new installation or update, immediately disable an
installed copy, and offer its removal. An absent or stale catalog never invents
a revocation. A catalog signature never bypasses package validation, user
consent, or runtime sandboxing.
