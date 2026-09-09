# OverCrow Widget Marketplace

This repository admits, packages, and publishes OverCrow Web API v1
extensions. The marketplace is open source under MIT. The OverCrow application,
Control Center, core, overlay, and built-in widgets are private, proprietary
software in a separate repository.

Extensions are local web apps: HTML, CSS, JavaScript or TypeScript, and
any web framework. WASM is optional compute, never a required UI engine.
OverCrow owns the outer chrome. The plugin owns its internal UX.

## Current status

The component/WIT/WASM product surface has been replaced with Web API
v1:

- `tools/marketplace-tool` validates the strict Web manifest, listing metadata,
  complete file ledger, and built bytes, then writes a deterministic stored-zip
  `.ocpkg`. Its admission commands bind each package to the validated
  `listing.json`, then persist and later re-verify both exact byte sequences in
  a private content-addressed store. Its development staging command signs a
  local catalog. Separate production prepare/finalize commands reserve an
  offline sequence, export exact payload bytes and verify a detached production
  signature without accepting a private key.
- `fixtures/hello-web` is the structural Web API v1 fixture.
- `widgets/warframe-market` is the reference extension: persistent
  controller, IndexedDB catalog and last query (~3840 structured items),
  `overcrow.fetch` to `api.warframe.market`, and a view that can
  hide/show without resetting search state. Cached catalog data is validated
  before reuse and remains searchable when refresh fails; successive selections
  cannot be replaced by older order responses. Item offers use the grouped
  `/v2/orders/item/{slug}/top` endpoint. The compact view indicates search
  focus, summarizes the displayed offers, and separates sellers from buyers.
  Each offer has an explicit whisper-copy action with pending, confirmed, and
  retry feedback; prices summarize the bounded rows shown, not the full market.
- `published/` is the tracked site output served by Coolify. Its UI can be
  staged independently; signed catalog and package bytes stay under
  `published/marketplace/v1/`.

Package locally:

```sh
node --test tests/warframe-market/market.test.mjs
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Pull-request admission is static and ephemeral. Trusted-push admission runs the
repository tests once, packages each widget once, and can ingest those exact
package and listing bytes into an explicitly supplied private store. Ingestion
re-inspects but does not execute, rebuild, or retest a package. Development
catalog staging reuses the stored bytes exactly. Runtime OverCrow verifies
signature, digest, and capabilities; it does not rebuild or retest widgets.
Production preparation and detached-signature finalization follow the
[offline operations runbook](docs/production-operations.md). They retain earlier
versions and revocations, verify the fixed production public key and never
publish or deploy. Private signing remains a separate external operation.
The generic maintainer sandbox for extension-defined build commands is not
implemented yet.

Declared browser `.wasm` assets are allowed for sandboxed page-side
computation. WIT/Wasmtime components and native executable modules remain
unsupported.

Production catalogs remain at
<https://overcrow.playervox.com/marketplace/v1/catalog.json> with a
90-day lifetime. Catalog publication requires the separate offline signing
procedure; UI changes do not require a new catalog signature.
Production private keys never enter this repository.
The published Web API v1 catalog currently offers Warframe Market. Historical
native-era package and preview URLs are retained, but those widgets are not
installable through the current catalog.

The catalog website provides compact preview cards, local search across names,
descriptions and authors, and an availability filter. Widget details use
`#widget/<canonical-id>` links; returning to the catalog preserves the current
search and filter. Full permissions, metadata, source and **Open in OverCrow**
are available on each detail view in English and French. Its
`overcrow://widget/<canonical-id>` links open widget details in the native
Control Center; they never install or activate a widget. Activation stays in
the in-game overlay library. The site displays the latest version per widget
using the same version ordering as the app, including a latest-version
revocation instead of falling back to an older verified entry.

Stage a website-only update with `node scripts/stage-marketplace-site.mjs`,
then run `node --test tests/site-runtime.test.js` and review the `published/`
diff before pushing. The command copies the marketplace UI and branding assets,
uses content-hashed script/style filenames, and never changes the signed
catalog or package tree. CI checks that the published shell matches the sources
and can render the current production catalog.

The site retains a display reader for historical native-era catalogs. These
entries are labeled **Legacy version**, excluded from the
Available filter and offer no native install link. Provider-only entries remain
hidden; their permissions are included in dependent widget details. This is
website display compatibility only; admission and the app still require Web API
v1; the published Web catalog does not use this compatibility path.

The marketplace uses the fixed dark-and-lime PlayerVox palette and component
styling defined in `playervox-front/src/index.css` and
`playervox-front/src/components/ui/`. It does not synchronize account theme
preferences. Its bundled Noto Sans fonts are served locally with their
[SIL Open Font License](web/marketplace/assets/NotoSans-OFL.txt). Catalog status
labels report catalog metadata; only OverCrow verifies signatures before
installation.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[review policy](docs/review-policy.md), and
[SECURITY.md](SECURITY.md) before proposing content.

## Licensing

Marketplace code, tooling, and documentation are licensed under [MIT](LICENSE).
PlayerVox widgets distributed separately, including `widgets/warframe-market`
and `fixtures/hello-web`, are also open source under their own MIT licenses.
Their bundled JavaScript SDK is MIT-licensed.

The license policy for third-party creators' widgets remains undecided. Neither
MIT nor the marketplace license is imposed on them by this policy. The existing
`spdxLicense` field records a package's declared license; format validation is
not a decision about which licenses will be accepted for publication.

See [LICENSING.md](LICENSING.md) for the scope, [NOTICE](NOTICE) for attribution,
and [TRADEMARKS.md](TRADEMARKS.md) for branding. Dependencies and external assets
retain their own licenses.
