# Contributing

Public arbitrary package submissions are not accepted yet. The initial local
proof of concept is maintainer-authored while the sandbox and publication
process undergo a separate security audit. This policy will be updated before
any public intake opens.

By submitting repository tooling or official widget source, you agree to
license your contribution under `AGPL-3.0-only` and confirm that you are
authorized to do so. Do not submit third-party code, assets, or data without a
documented origin and compatible license.

Every proposed package must declare:

1. a valid SPDX license identifier;
2. its author or responsible maintainer;
3. a canonical source URL;
4. one default locale;
5. the exact available locale list;
6. every requested capability;
7. its exact game scope;
8. every dependency by immutable ID, version, and digest.

The default locale is mandatory; additional translations are optional for
community creators. Available languages must remain visible on the listing.

Before a package can enter a catalog, automated checks and the complete
[review policy](docs/review-policy.md) must pass, followed by explicit human
maintainer approval and signing. Passing automation is not a security audit or
an entitlement to publication.

Keep changes focused and run:

```sh
sh -n scripts/check-policy.sh
scripts/check-policy.sh
```

Contributors retain copyright in their work. No copyright assignment or
contributor license agreement is currently required.
