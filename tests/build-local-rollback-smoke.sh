#!/bin/sh
set -eu
repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-rollback.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM
git clone --quiet --no-hardlinks "$repo_root" "$scratch/repository"
copy="$scratch/repository"
cp "$repo_root/scripts/build-local.sh" "$copy/scripts/build-local.sh"
ln -s "$repo_root/target" "$copy/target"
mkdir -p "$copy/public"
printf '%s\n' prior >"$copy/public/prior"
before=$(/usr/bin/sha256sum "$copy/providers/warframe-worldstate/manifest.json")
if MARKETPLACE_TEST_FAIL_AFTER_MOVE=1 sh "$copy/scripts/build-local.sh"; then
    printf '%s\n' 'error: fault injection unexpectedly succeeded' >&2
    exit 1
fi
test "$(/usr/bin/sha256sum "$copy/providers/warframe-worldstate/manifest.json")" = "$before"
test -f "$copy/public/prior"
test ! -e "$copy/.public-next" && test ! -e "$copy/.public-previous"
if /usr/bin/find "$copy" -maxdepth 1 -name '.public-*' -print -quit | grep .; then
    printf '%s\n' 'error: publication transient remains' >&2
    exit 1
fi
