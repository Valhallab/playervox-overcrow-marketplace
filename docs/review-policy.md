# Package Review Policy

Every catalog entry requires both automated validation and explicit human
maintainer approval. Automation is evidence, not a security certification.
Submissions live at `community/<publisher>/<widget-id>/` and target
`candidate`. CI has read-only repository access and no credential or signing
authority. Hosted CI performs static admission only and never executes submitted
code; the maintainer's complete gate performs compilation and tests inside the
documented sandbox before acceptance or promotion.

Reviewers must verify:

- strict manifest and schema validation, including stable identity, exact
  versions, immutable dependency digests, locale declarations, and game scope;
- source and asset provenance plus an SPDX-compatible license review;
- a deterministic build whose package and catalog outputs reproduce exactly;
- malicious fixtures for archive, manifest, component, image, locale, IPC,
  capability, and size boundaries relevant to the package;
- a plain-language explanation of every requested permission and dependency;
- a zero-import component inspection and the supported sandbox test gate;
- an exact reviewed revision, maintainer approval record, and maintainer
  signature before catalog inclusion.

Reviewers also confirm that declared rendering uses only the supported native
nodes (rows, columns, grids, scroll regions, text, host icons, bounded raster
images, buttons, toggles, text inputs, selections, lists, progress, charts,
and bounded 2D canvas) and receives only the documented semantic events. They
must check every capability: exact HTTPS hosts, `overcrow.session.v1` game
data, private storage, clipboard write, and provider publication. A package
must remain inside the documented manifest, listing, component, archive,
entry, asset, preview, locale, dependency, and host limits, and declare an
exact default locale with a safe fallback.

For a user-visible package with a hidden provider, review the exact dependency
ID, version, and digest and present the combined permission set in plain
language. A provider is disclosed in details and consent but never becomes a
normal browsing card. Catalog inclusion requires a reproducible local build,
valid development signature for development work, and a separate human
approval; it never authorizes production signing.

Review fails closed on missing evidence, ambiguous ownership, mutable sources,
unreviewed capability expansion, unknown files, or inconsistent metadata. A
later update repeats the applicable review; prior approval does not cover new
code or permissions.

Merge into `candidate` records review acceptance only. Publication is a later
offline maintainer promotion of an exact accepted revision; creators never
receive the catalog signing key or deployment authority.

Suspension and revocation use newly signed, monotonically newer catalog data.
No reviewer may edit an installed user's registry or advise bypassing host
validation.
