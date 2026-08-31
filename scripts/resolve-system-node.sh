#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: resolve-system-node.sh' >&2
    exit 2
fi

node_candidate=$(command -v node 2>/dev/null || :)
case "$node_candidate" in
    /*) ;;
    *) node_candidate='' ;;
esac
node_path=$(/usr/bin/readlink -f -- "$node_candidate" 2>/dev/null || :)
if test -z "$node_path" || test "$node_candidate" != "$node_path" \
        || test ! -f "$node_path" || test -L "$node_path" \
        || test ! -x "$node_path" \
        || test "$(/usr/bin/stat -c '%u:%h' "$node_path" 2>/dev/null || :)" \
            != 0:1 \
        || test "$(/usr/bin/stat -c '%s' "$node_path" 2>/dev/null || :)" \
            -gt 268435456 \
        || /usr/bin/find "$node_path" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: trusted system Node is unavailable' >&2
    exit 1
fi

node_directory=${node_path%/*}
while :; do
    if test ! -d "$node_directory" || test -L "$node_directory" \
            || test "$(/usr/bin/stat -c '%u' "$node_directory" \
                2>/dev/null || :)" != 0 \
            || /usr/bin/find "$node_directory" -maxdepth 0 -perm /0022 \
                -print -quit | /usr/bin/grep . >/dev/null; then
        printf '%s\n' 'error: trusted system Node is unavailable' >&2
        exit 1
    fi
    test "$node_directory" = / && break
    node_parent=${node_directory%/*}
    test -n "$node_parent" || node_parent=/
    if test "$node_parent" = "$node_directory"; then
        printf '%s\n' 'error: trusted system Node is unavailable' >&2
        exit 1
    fi
    node_directory=$node_parent
done

printf '%s\n' "$node_path"
