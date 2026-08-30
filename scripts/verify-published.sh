#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3; then
    printf '%s\n' 'usage: verify-published.sh ABSOLUTE-TREE ABSOLUTE-PUBLIC-KEY overcrow-production-2026-01' >&2
    exit 2
fi
tree=$1
public_key=$2
key_id=$3
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(/usr/bin/dirname -- "$script_dir")
case "$tree:$public_key" in /*:/*) ;; *)
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
    ;;
esac
if test "$key_id" != overcrow-production-2026-01 \
        || test ! -d "$tree" || test -L "$tree" \
        || test "$(CDPATH='' cd -- "$tree" && pwd -P)" != "$tree"; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

work=$(/usr/bin/mktemp -d /tmp/marketplace-verify.XXXXXXXXXX)
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
tool_work="$work/tool"
/usr/bin/install -d -m 0700 "$tool_work"
trusted_tool=$(sh "$script_dir/prepare-marketplace-tool.sh" \
    "$repo_root" "$tool_work" 2>/dev/null) || {
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
}
catalog="$tree/marketplace/v1/catalog.json"
if ! "$trusted_tool" verify "$catalog" --public-key "$public_key" \
        --key-id "$key_id" >/dev/null 2>&1 \
        || ! "$trusted_tool" verify-tree --repository "$repo_root" \
            --tree "$tree" --public-key "$public_key" --key-id "$key_id" \
            >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

node_path=$(command -v node 2>/dev/null || :)
node_path=$(/usr/bin/readlink -f -- "$node_path" 2>/dev/null || :)
if test "$node_path" = /usr/bin/mise; then
    node_path=$(/usr/bin/mise which node 2>/dev/null || :)
    node_path=$(/usr/bin/readlink -f -- "$node_path" 2>/dev/null || :)
fi
if test -z "$node_path" || test ! -f "$node_path" || test -L "$node_path" \
        || test ! -x "$node_path" \
        || /usr/bin/find "$node_path" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi
for landing_test in effects.test.mjs landing-content.test.mjs static-hygiene.test.mjs; do
    if ! /usr/bin/env -i PATH="${node_path%/*}:/usr/bin:/bin" LC_ALL=C.UTF-8 \
            "$node_path" "$repo_root/tests/landing/$landing_test" "$tree" \
            >/dev/null 2>&1; then
        printf '%s\n' 'error: published tree rejected' >&2
        exit 1
    fi
done
if ! /usr/bin/env -i PATH="${node_path%/*}:/usr/bin:/bin" LC_ALL=C.UTF-8 \
        MARKETPLACE_CATALOG_PATH="$catalog" \
        "$node_path" --test \
            --test-name-pattern='production mode renders complete catalog metadata' \
            "$repo_root/tests/site-runtime.test.js" >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi
