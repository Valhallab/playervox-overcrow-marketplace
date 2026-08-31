## Widget submission

Publisher and widget ID: `community/<publisher>/<widget-id>/`

Describe the widget, its intended game or use, and the exact revision tested.

## Review declarations

- [ ] Every requested capability is listed with its user-facing reason.
- [ ] Every HTTPS host is exact and contains no credentials, wildcard, path, or redirect dependency.
- [ ] Game scope and all provider dependencies are listed.
- [ ] Source, assets, fonts, preview, and third-party licenses have documented provenance.
- [ ] The creator crate has no `build.rs`, proc-macro target, Git dependency, custom registry, or dependency outside the reviewed policy.
- [ ] English metadata is present; every additional locale is listed and tested.
- [ ] The exact submission root is listed in the Cargo workspace and `marketplace/targets.json`.
- [ ] The local `.ocpkg` installs disabled and uninstalls with its unreferenced dependencies and private data removed.
- [ ] This PR targets `candidate` and does not modify `published/`.

## Evidence

List the exact local commands run and their results. Note any supported-desktop
or game checks that still require maintainer verification.
