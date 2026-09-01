#!/bin/sh
set -eu
umask 077

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-publisher-bundle.XXXXXXXXXX)
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$scratch"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

fixture="$scratch/release"
/usr/bin/install -d -m 0700 -- "$fixture/scripts" "$fixture/keys"
/usr/bin/cp "$repo_root/scripts/build-production.sh" "$fixture/scripts/"
/usr/bin/cp "$repo_root/scripts/review-bundle.sh" "$fixture/scripts/"
/usr/bin/chmod 0755 "$fixture/scripts/"*.sh
printf '%s\n' '[workspace]' >"$fixture/Cargo.toml"
printf '%s\n' fixture >"$fixture/keys/overcrow-production-2026-01.pub"
/usr/bin/git init --quiet "$fixture"
/usr/bin/git -C "$fixture" config user.name 'Marketplace Publisher Tests'
/usr/bin/git -C "$fixture" config user.email 'publisher-tests@invalid.example'
/usr/bin/git -C "$fixture" checkout --quiet -b release/fixture
/usr/bin/git -C "$fixture" add --all
/usr/bin/git -C "$fixture" commit --quiet -m 'publisher fixture'
revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)

if (
    cd "$fixture"
    sh scripts/build-production.sh \
        --candidate-revision "$revision" \
        --review-bundle "$scratch/missing.bundle" \
        --sequence-file "$scratch/sequence.txt" \
        --sequence-state "$scratch/state.json" \
        --signing-key "$scratch/signing.key" \
        --public-key "$fixture/keys/overcrow-production-2026-01.pub" \
        --key-id overcrow-production-2026-01
) >"$scratch/stdout" 2>"$scratch/stderr"; then
    printf '%s\n' 'error: publisher accepted a missing review bundle' >&2
    exit 1
fi
test ! -s "$scratch/stdout"
test "$(/usr/bin/cat "$scratch/stderr")" = \
    'error: production review bundle rejected'

printf '%s\n' 'Production review-bundle smoke test passed'
