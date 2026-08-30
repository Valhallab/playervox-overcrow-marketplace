#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3; then
    printf '%s\n' 'usage: extract-component-artifacts.sh ARCHIVE BUILD-PLAN OUTPUT' >&2
    exit 2
fi
archive=$1
build_plan=$2
output=$3
invoking_uid=$(/usr/bin/id -u)
if test ! -f "$archive" || test -L "$archive" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$archive")" \
            != "$invoking_uid:600:1" \
        || test "$(/usr/bin/stat -c '%s' "$archive")" -gt 33554432 \
        || test ! -f "$build_plan" || test -L "$build_plan" \
        || test -e "$output" || test -L "$output"; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi

listing=$(/usr/bin/mktemp /tmp/marketplace-artifacts-list.XXXXXXXXXX) || exit 1
verbose=$(/usr/bin/mktemp /tmp/marketplace-artifacts-verbose.XXXXXXXXXX) || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$listing" "$verbose"
    if test "$status" -ne 0; then
        /usr/bin/rm -rf -- "$output"
    fi
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nofile=64 --fsize=1048576 -- \
        /usr/bin/tar --list --file="$archive" --quoting-style=escape \
            >"$listing" 2>/dev/null \
        || ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
            /usr/bin/timeout --signal=KILL 10 \
            /usr/bin/prlimit --cpu=5 --as=536870912 --nofile=64 --fsize=1048576 -- \
            /usr/bin/tar --list --verbose --numeric-owner --full-time \
                --file="$archive" --quoting-style=escape >"$verbose" 2>/dev/null; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi

tab=$(printf '\t')
expected_count=0
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    expected="$component_artifact.wasm"
    if test "$(/usr/bin/grep -F -x -c -- "$expected" "$listing" || :)" != 1; then
        printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
        exit 1
    fi
    expected_count=$((expected_count + 1))
done <"$build_plan"
if test "$expected_count" -eq 0 \
        || test "$(/usr/bin/wc -l <"$listing")" != "$expected_count" \
        || test "$(/usr/bin/wc -l <"$verbose")" != "$expected_count" \
        || ! /usr/bin/awk '
            substr($1, 1, 1) != "-" { exit 1 }
            $3 !~ /^[0-9]+$/ || $3 > 4194304 { exit 1 }
            END { if (NR == 0) exit 1 }
        ' "$verbose"; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi

/usr/bin/install -d -m 0700 -- "$output"
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nofile=64 --fsize=4194304 -- \
        /usr/bin/tar --extract --file="$archive" --directory="$output" \
            --no-same-owner --no-same-permissions --no-xattrs --no-acls \
            --no-selinux --no-overwrite-dir --keep-directory-symlink; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi
artifact_count=0
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    artifact="$output/$component_artifact.wasm"
    if test ! -f "$artifact" || test -L "$artifact" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$artifact")" \
                != "$invoking_uid:600:1" \
            || test "$(/usr/bin/stat -c '%s' "$artifact")" -gt 4194304; then
        printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
        exit 1
    fi
    artifact_count=$((artifact_count + 1))
done <"$build_plan"
if test "$artifact_count" != "$expected_count" \
        || test "$(/usr/bin/find "$output" -mindepth 1 -maxdepth 1 -type f -printf . \
            | /usr/bin/wc -c)" != "$expected_count" \
        || test -n "$(/usr/bin/find "$output" -mindepth 1 ! -type f -print -quit)"; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi
