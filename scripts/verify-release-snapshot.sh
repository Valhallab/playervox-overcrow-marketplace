#!/bin/sh
set -eu
umask 077

reject() {
    printf '%s\n' 'error: release snapshot rejected' >&2
    exit 1
}

if test "$#" -ne 8; then
    printf '%s\n' \
        'usage: verify-release-snapshot.sh TRUSTED-ROOT HEAD-ROOT TRUSTED-TOOL EVENT REPOSITORY BASE-REF HEAD-REPOSITORY HEAD-REF' >&2
    exit 2
fi
trusted_root=$1
head_root=$2
trusted_tool=$3
event_name=$4
repository=$5
base_ref=$6
head_repository=$7
head_ref=$8

case "$trusted_root:$head_root:$trusted_tool" in
    /*:/*:/*) ;;
    *) reject ;;
esac
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || reject
invoking_uid=$(/usr/bin/id -u)
trusted_key="$trusted_root/keys/overcrow-production-2026-01.pub"
head_key="$head_root/keys/overcrow-production-2026-01.pub"
trusted_key_metadata=$(
    /usr/bin/stat -c '%u:%a:%h:%s' "$trusted_key" 2>/dev/null || :
)
head_key_metadata=$(
    /usr/bin/stat -c '%u:%a:%h:%s' "$head_key" 2>/dev/null || :
)
case "$trusted_key_metadata" in
    "$invoking_uid:600:1:65" | "$invoking_uid:644:1:65") ;;
    *) reject ;;
esac
if test "$trusted_root" = / || test "$head_root" = / \
        || test "$trusted_root" = "$head_root" \
        || test "$script_dir" != "$trusted_root/scripts" \
        || test ! -d "$trusted_root" || test -L "$trusted_root" \
        || test "$(CDPATH='' cd -- "$trusted_root" && pwd -P)" != "$trusted_root" \
        || test ! -d "$head_root" || test -L "$head_root" \
        || test "$(CDPATH='' cd -- "$head_root" && pwd -P)" != "$head_root" \
        || test ! -f "$trusted_tool" || test -L "$trusted_tool" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$trusted_tool" 2>/dev/null || :)" \
            != "$invoking_uid:700:1" \
        || test ! -f "$trusted_root/tests/reject-published-change.sh" \
        || test -L "$trusted_root/tests/reject-published-change.sh" \
        || test ! -f "$trusted_key" || test -L "$trusted_key" \
        || test ! -f "$head_key" || test -L "$head_key" \
        || test "$head_key_metadata" != "$trusted_key_metadata" \
        || ! /usr/bin/cmp --silent "$trusted_key" "$head_key"; then
    reject
fi

if ! sh "$trusted_root/tests/reject-published-change.sh" \
        "$event_name" "$repository" "$base_ref" \
        "$head_repository" "$head_ref" published >/dev/null 2>&1 \
        || ! "$trusted_tool" verify-release-snapshot \
            --trusted-repository "$trusted_root" \
            --head-repository "$head_root" \
            --public-key "$trusted_key" \
            --key-id overcrow-production-2026-01 >/dev/null 2>&1; then
    reject
fi

printf '%s\n' 'Trusted release snapshot verified'
