#!/bin/sh
set -eu
umask 077

cargo test -p marketplace-tool --locked
node --test tests/warframe-market/market.test.mjs
node --test tests/site-runtime.test.js
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cargo run -p marketplace-tool --locked --quiet -- package fixtures/hello-web "$tmp/hello.ocpkg"
cargo run -p marketplace-tool --locked --quiet -- inspect "$tmp/hello.ocpkg" >/dev/null
cargo run -p marketplace-tool --locked --quiet -- package widgets/warframe-market "$tmp/warframe-market.ocpkg"
cargo run -p marketplace-tool --locked --quiet -- inspect "$tmp/warframe-market.ocpkg" >/dev/null
