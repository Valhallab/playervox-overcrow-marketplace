# Security Policy

## Reporting a vulnerability

Do not open a public issue or publish a proof-of-concept exploit for an
unresolved vulnerability. Use
[GitHub Private Vulnerability Reporting](https://github.com/Valhallab/playervox-overcrow/security/advisories/new)
to contact Valhallab SASU privately and identify this marketplace as the
affected project. Include the affected revision, impact, environment, and the
smallest safe reproduction. Never include user data, credentials, production
keys, or provider payloads.

Coordinated disclosure is appreciated. The project does not currently offer a
bug bounty or guarantee a response deadline.

## Launch and incident boundary

Arbitrary public submissions and production publication remain disabled until
a separate security audit, fuzzing, dependency review, supported-distribution
sandbox validation, key-operations procedure, and incident runbook are
complete.

If a listed package is suspected of compromise, maintainers may publish a
signed suspension or revocation in a newer monotonic catalog. Clients must
reject that package for new installation or update. A catalog signature never
bypasses package validation, user consent, or runtime sandboxing.
