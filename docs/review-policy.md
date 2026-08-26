# Package Review Policy

Every catalog entry requires both automated validation and explicit human
maintainer approval. Automation is evidence, not a security certification.

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

Review fails closed on missing evidence, ambiguous ownership, mutable sources,
unreviewed capability expansion, unknown files, or inconsistent metadata. A
later update repeats the applicable review; prior approval does not cover new
code or permissions.

Suspension and revocation use newly signed, monotonically newer catalog data.
No reviewer may edit an installed user's registry or advise bypassing host
validation.
