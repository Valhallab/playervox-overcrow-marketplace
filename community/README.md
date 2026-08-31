# Community widget submissions

Submit each widget as one self-contained reviewed source tree at
`community/<publisher>/<widget-id>/`. Publisher and widget identifiers use
1–63 lowercase ASCII letters, digits, and hyphens, start and end with a letter
or digit, and must remain stable across updates.

The tree includes the Rust crate and source, manifest, listing, locale files,
tests, preview, package license, third-party license notices, and provenance
for every source and asset. English metadata is required. Translations are
optional, but the manifest must list every supplied locale exactly and each
listed locale must be tested. Dependencies must come from the reviewed policy;
creator crates cannot define `build.rs` or a proc-macro target and cannot use
Git dependencies or custom registries.

Add the exact submission root to the Cargo workspace and to
`marketplace/targets.json`; CI rejects an extant changed root missing either
connection. The target schema's `verified` status describes the requested
review outcome. Its presence in an unmerged pull request is not maintainer
acceptance and never means publication.

Work from a fork or short-lived branch and open the pull request against
`candidate` with the widget submission template completed. CI supplies
read-only automated evidence for policy, formatting, lint, tests, component
inspection, sandboxing, packaging, and deterministic output. A maintainer then
reviews the exact source revision, capability reasons, HTTPS hosts, game scope,
dependency closure, licenses, provenance, preview, and user-facing text.

Merging into `candidate` records review acceptance; it does not publish the
widget. A maintainer may later promote an exact accepted revision through the
offline release process. Creators receive no signing key, upload token,
deployment credential, or publication authority. Every update repeats the
applicable automated and human review.

Test a local `.ocpkg` through the Control Center's explicit unverified install
flow. It installs disabled and remains disabled until the user enables it. Its
uninstall path must remove unreferenced dependencies and private data. The
website displays catalog information only and never installs packages.

Read the [creator guide](../docs/creator-guide.md),
[review policy](../docs/review-policy.md), and
[security policy](../SECURITY.md) before submitting.
