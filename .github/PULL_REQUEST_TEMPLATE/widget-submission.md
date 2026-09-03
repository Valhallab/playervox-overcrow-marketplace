## Extension submission

Directory: `widgets/<extension-id>/` (or another reviewed web directory)

Describe the extension, its intended game or use, and the exact revision tested.

## Review declarations

- [ ] The package is a Web API v1 web app (`manifest.json` + `listing.json` + declared files).
- [ ] There is no WIT, Wasmtime, native module, provider graph, or `component.wasm`.
- [ ] Every HTTPS grant is exact (`origin`, `method`, `pathPrefix`) and contains no credentials or wildcards.
- [ ] Source, assets, fonts, preview, and third-party licenses have documented provenance.
- [ ] English listing metadata is present; every additional locale is listed.
- [ ] `marketplace-tool package` and `inspect` succeed on this directory.
- [ ] This PR targets `candidate` and does not modify `published/`.

## Evidence

List the exact local commands run and their results. Note any supported-desktop
or game checks that still require maintainer verification.
