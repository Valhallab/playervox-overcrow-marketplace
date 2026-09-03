# Creator guide

An OverCrow extension is a local web app.

1. Write HTML/CSS/JavaScript or TypeScript with any framework.
2. Declare a Web API v1 `manifest.json`: identity, `entrypoints.view`,
   optional controller, exact HTTPS network grants, and a file ledger
   of SHA-256 plus byte length for every packaged file except
   `manifest.json`.
3. Declare public listing metadata in `listing.json`.
4. During development, point an installed OverCrow at the folder or a
   localhost/Vite server. Do not package, sign, or publish to iterate.
5. When ready, `marketplace-tool package` writes a deterministic
   `.ocpkg`. Marketplace admission reuses that archive.

The host exposes `overcrow.*`. Page code cannot reach processes, game
memory, arbitrary files, Node, Tauri, or native modules. Network access
goes through `overcrow.fetch` to declared HTTPS endpoints only.
