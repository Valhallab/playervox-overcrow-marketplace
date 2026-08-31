#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: build-local.sh' >&2
    exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(/usr/bin/dirname -- "$script_dir")
if test ! -f "$repo_root/Cargo.toml" || test -L "$repo_root"; then
    printf '%s\n' 'error: marketplace repository is unavailable or unsafe' >&2
    exit 1
fi

exec 9<"$repo_root"
if ! /usr/bin/flock -n 9; then
    printf '%s\n' 'error: a local marketplace build is already running' >&2
    exit 1
fi

stage=$(/usr/bin/mktemp -d "$repo_root/.build-local.XXXXXXXXXX") || exit 1
source_root="$stage/repository"
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$stage"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

sh "$script_dir/stage-catalog-repository.sh" --mode development "$source_root"
/usr/bin/rm -f -- "$source_root/marketplace/development-catalog-state.json"
tool_work="$stage/trusted-tool"
/usr/bin/install -d -m 0700 "$tool_work"
trusted_tool=$(sh "$script_dir/prepare-marketplace-tool.sh" \
    "$repo_root" "$tool_work")
cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
    --locked --quiet -- build \
    --repository "$source_root" \
    --generated-at 2026-08-27T00:00:00Z \
    --expires-at 2036-08-27T00:00:00Z \
    --development-key
cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
    --locked --quiet -- verify "$source_root/public/marketplace/v1/catalog.json"

if test ! -d "$source_root/public" || test -L "$source_root/public"; then
    printf '%s\n' 'error: generated catalog tree is unavailable or unsafe' >&2
    exit 1
fi
/usr/bin/cp -R -- "$repo_root/web/landing/." "$source_root/public/"
/usr/bin/install -d -m 0755 "$source_root/public/marketplace"
for file in index.html app.js styles.css; do
    /usr/bin/install -m 0644 "$repo_root/web/marketplace/$file" \
        "$source_root/public/marketplace/$file"
done
/usr/bin/install -m 0644 "$repo_root/web/marketplace/policies/development.js" \
    "$source_root/public/marketplace/catalog-policy.js"

next_public="$repo_root/.public-next.$$"
previous_public="$repo_root/.public-previous.$$"
sh "$script_dir/publish-directory.sh" \
    "$source_root/public" "$repo_root/public" "$next_public" "$previous_public" \
    /usr/bin/true /usr/bin/true /usr/bin/true /usr/bin/true "$trusted_tool"
