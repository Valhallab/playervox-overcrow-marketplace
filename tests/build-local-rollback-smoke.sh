#!/bin/sh
set -eu
repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-rollback.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM
git clone --quiet --no-hardlinks "$repo_root" "$scratch/repository"
copy="$scratch/repository"
helper="$repo_root/scripts/publish-directory.sh"
tracked_paths="$scratch/tracked-paths"
tracked_before="$scratch/tracked-before"
tracked_after="$scratch/tracked-after"
git -C "$copy" ls-files '*manifest.json' marketplace/development-catalog-state.json \
    >"$tracked_paths"
snapshot_tracked() {
    destination=$1
    (
        cd "$copy"
        while IFS= read -r path; do
            /usr/bin/sha256sum "$path"
        done <"$tracked_paths"
    ) >"$destination"
}

/usr/bin/mkdir -p "$copy/public/nested"
printf '%s\n' prior >"$copy/public/prior"
printf '%s\n' 'nested prior bytes' >"$copy/public/nested/file with spaces"
/usr/bin/cp -a -- "$copy/public" "$scratch/prior-public"
snapshot_tracked "$tracked_before"

next_public="$copy/.public-next.rollback"
previous_public="$copy/.public-previous.rollback"
/usr/bin/mkdir -p "$next_public"
printf '%s\n' next >"$next_public/next"
if sh "$helper" "$next_public" "$copy/public" "$previous_public" \
        /usr/bin/mv /usr/bin/false /usr/bin/mv; then
    printf '%s\n' 'error: failing publication move unexpectedly succeeded' >&2
    exit 1
fi
/usr/bin/diff --recursive --no-dereference "$scratch/prior-public" "$copy/public"
test ! -L "$copy/public"
test "$(CDPATH='' cd -- "$copy/public" && pwd -P)" = "$copy/public"
test ! -e "$next_public" && test ! -L "$next_public"
test ! -e "$previous_public" && test ! -L "$previous_public"
snapshot_tracked "$tracked_after"
/usr/bin/cmp -s -- "$tracked_before" "$tracked_after"

next_public="$copy/.public-next.restore-failure"
previous_public="$copy/.public-previous.restore-failure"
/usr/bin/mkdir -p "$next_public"
printf '%s\n' next >"$next_public/next"
if sh "$helper" "$next_public" "$copy/public" "$previous_public" \
        /usr/bin/mv /usr/bin/false /usr/bin/false; then
    printf '%s\n' 'error: failed publication and restoration unexpectedly succeeded' >&2
    exit 1
fi
test ! -e "$copy/public" && test ! -L "$copy/public"
test ! -e "$next_public" && test ! -L "$next_public"
test -d "$previous_public" && test ! -L "$previous_public"
/usr/bin/diff --recursive --no-dereference "$scratch/prior-public" "$previous_public"
/usr/bin/mv -- "$previous_public" "$copy/public"
test ! -e "$previous_public" && test ! -L "$previous_public"
/usr/bin/diff --recursive --no-dereference "$scratch/prior-public" "$copy/public"
snapshot_tracked "$tracked_after"
/usr/bin/cmp -s -- "$tracked_before" "$tracked_after"
