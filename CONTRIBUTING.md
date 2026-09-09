# Contributing

Contributions to marketplace code, tooling, and documentation are accepted
under [MIT](LICENSE). Submit only material you are authorized to license under
those terms, and document the origin and licenses of third-party material.
Contributors retain copyright in their work.

Widgets have a separate licensing scope. PlayerVox widgets distributed
separately are MIT-licensed. The license policy for third-party creators'
widgets remains undecided; submitting a widget does not automatically place it
under MIT or the marketplace license. Declare its actual license in
`listing.json`, preserve applicable notices, and consult [LICENSING.md](LICENSING.md).
Technical validation is not approval of a license for publication.

Submit Web API v1 extensions only. A submission is a directory of web
files plus `manifest.json` and `listing.json`. Do not send WIT worlds,
Wasmtime components, native modules, or provider graphs.
Declared browser `.wasm` assets are permitted only as page-side computation
inside the normal Web sandbox; they do not receive native or system authority.

## Local checks

```sh
cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
tests/catalog-stage-smoke.sh
cargo run -p marketplace-tool --locked -- package widgets/warframe-market /tmp/warframe-market.ocpkg
cargo run -p marketplace-tool --locked -- inspect /tmp/warframe-market.ocpkg
```

Hosted pull-request CI packages the exact proposed Git tree with the
base-reviewed tool without executing proposed code. Push CI tests the exact
trusted revision. See `SECURITY.md` for the trust boundary and
`docs/testing.md` for the covered scenarios. An operator may explicitly persist
a trusted-push admission in a private store; later consumers re-verify and reuse
that exact artifact. The generic sandbox for extension-defined build commands
is not implemented, so submissions currently include their built web files.

Do not commit production keys, `.ocpkg` outputs outside fixtures, or
changes under `published/` unless a later authorized publication task
asks for them.
