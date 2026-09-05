# Creator guide

An OverCrow extension is a local web app.

1. Write HTML/CSS/JavaScript or TypeScript with any framework and run its
   static build yourself.
2. Put Web API v1 `manifest.json` metadata in that build: identity,
   `entrypoints.view`, optional controller, and explicit permissions. The
   `files` ledger may be omitted here.
3. Prepare a fresh runtime directory with the app's creator CLI:

   ```sh
   mkdir -m 700 /absolute/path/to/widget-output
   overcrow-widget prepare /absolute/path/to/static-build /absolute/path/to/widget-output/build-1
   overcrow-widget dev /absolute/path/to/widget-output/build-1
   ```

   `prepare` copies the static assets and computes their SHA-256 and byte
   lengths. It replaces only the output ledger, never the source manifest or
   assets. Permissions stay explicit and receive no automatic expansion. It
   never runs builds or installs anything. The output must not exist, must be
   outside the source tree, and needs an existing private `0700` parent. Both
   paths must be absolute without symlink components. Use a fresh output name
   after rebuilding. Native executables/modules, unsafe paths, symlinks,
   hardlinks, and shared-writable input files are rejected.
4. The prepared directory is accepted by the existing strict `dev` and
   `overcrow-widget package` readers. For a numeric-loopback server, serve the
   prepared directory at an explicit address such as `http://127.0.0.1:4173`
   and use `overcrow-widget dev --url http://127.0.0.1:4173`. The served ledger
   must match its bytes; `localhost` is not accepted. Keep source code, build
   tooling, and listing metadata outside the runtime directory.
5. Copy the prepared bundle into a separate review directory, then add your
   reviewed public listing metadata as root `listing.json`:

   ```sh
   cp -R /absolute/path/to/widget-output/build-1 /absolute/path/to/widget-output/review-1
   cp /absolute/path/to/reviewed-listing.json /absolute/path/to/widget-output/review-1/listing.json
   marketplace-tool package /absolute/path/to/widget-output/review-1 /absolute/path/to/widget-output/widget.ocpkg
   ```

   Marketplace packaging validates this sidecar and omits it from the
   deterministic `.ocpkg`. If the original static build contains a regular
   root `listing.json` of at most 64 KiB, `prepare` excludes it without
   interpreting or approving it; review and marketplace validation remain
   explicit. A nested `listing.json` is an ordinary runtime asset and enters
   the generated ledger. Marketplace admission reuses the validated archive.
   Development requires no packaging, signing, or publication.

Preparation retains the runtime package ceilings: 128 MiB and 4,096 entries
including `manifest.json`, 192-byte portable ASCII paths, and 1 MiB manifest
metadata. Source traversal additionally bounds all files and directories to
8,192 entries. A prepared bundle conveys no signing or marketplace trust.

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
