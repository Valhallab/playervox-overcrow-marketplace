# Creator guide

An OverCrow extension is a local web app.

1. Write HTML/CSS/JavaScript or TypeScript with any framework.
2. Declare a Web API v1 `manifest.json`: identity, `entrypoints.view`,
   optional controller, exact HTTPS network grants, and a file ledger
   of SHA-256 plus byte length for every packaged file except
   `manifest.json`.
3. During development, pass the built static bundle to
   `overcrow-widget dev /absolute/path/to/widget`, or serve it at an explicit
   numeric-loopback address such as `http://127.0.0.1:4173` and use
   `overcrow-widget dev --url http://127.0.0.1:4173`. The file ledger must match
   the served bytes. `localhost` is not accepted. Keep source files, build
   tooling, and marketplace listing metadata outside the bundle directory.
4. When ready for review, copy the bundle into a review directory and add
   public listing metadata as a regular root `listing.json`. This sidecar
   belongs to marketplace admission, not the manifest file ledger or runtime
   bundle. A nested `listing.json` is an ordinary asset and must be declared.
5. Run `marketplace-tool package /absolute/path/to/review /tmp/widget.ocpkg`.
   It validates the listing sidecar and omits it from the deterministic
   `.ocpkg`. Marketplace admission reuses that archive. Development itself
   requires no packaging, signing, or publication.

The host exposes `overcrow.*`. Page code cannot reach processes, game
memory, arbitrary files, Node, Tauri, or native modules. Network access
goes through `overcrow.fetch` to declared HTTPS endpoints only.
Optional browser WebAssembly may be declared in the file ledger for local
computation; it stays inside the same WebKit sandbox and has no host ABI.
Known Web assets receive their standard MIME type. Other declared regular data
is served as `application/octet-stream` with content sniffing disabled, so a
framework may ship opaque data without creating a new executable file class.
Page code may read those verified same-bundle files with browser APIs; external
HTTP(S) remains available only through `overcrow.fetch` and exact grants.
