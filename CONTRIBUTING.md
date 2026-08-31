# Contributing

Reviewed community package submissions are accepted. Work in a fork or
short-lived branch, place one source tree at
`community/<publisher>/<widget-id>/`, complete the widget submission template,
and open a pull request to `candidate`. See the
[community submission contract](community/README.md) for the required tree.

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

English metadata is mandatory. Additional translations are optional, and the
manifest must list every locale actually supplied so the marketplace can show
the exact available languages.

Hosted CI produces read-only static admission evidence without compiling or
executing submitted code. A maintainer then runs the sandboxed build gate and
applies the complete [review policy](docs/review-policy.md) to the exact
revision. Merge to `candidate` records review acceptance but does not publish
the package. A separate maintainer-owned offline promotion is required;
creators never receive signing or deployment credentials. Updates repeat the
applicable review.

Test a local `.ocpkg` through the Control Center's explicit unverified flow. It
must install disabled, remain disabled until enabled by the user, and uninstall
with unreferenced dependencies and private data removed. The website cannot
install packages.

Keep changes focused and run:

```sh
sh -n scripts/check-policy.sh
scripts/check-policy.sh
```

Contributors retain copyright in their work. No copyright assignment or
contributor license agreement is currently required.
