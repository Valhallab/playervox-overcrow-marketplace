#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: admission-store-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-admission-store.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

artifacts="$scratch/artifacts"
store="$scratch/accepted"
/usr/bin/install -d -m 0700 -- "$artifacts" "$store"
package_output=$(cargo run -p marketplace-tool --locked --quiet -- \
    package "$repo_root/fixtures/hello-web" "$artifacts/1.ocpkg")
digest=${package_output%% *}
bytes=$(/usr/bin/stat -c '%s' "$artifacts/1.ocpkg")
trust_sha=1111111111111111111111111111111111111111
review_sha=2222222222222222222222222222222222222222
review_tree=3333333333333333333333333333333333333333
receipt="$scratch/admission.tsv"
printf 'admission\t1\t%s\t%s\t%s\nartifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t1.0.0\t%s\t%s\n' \
    "$trust_sha" "$review_sha" "$review_tree" "$digest" "$bytes" >"$receipt"

if ! ingested=$(cargo run -p marketplace-tool --locked --quiet -- \
        ingest --receipt "$receipt" --artifacts "$artifacts" --store "$store" \
        --trust-sha "$trust_sha" --review-sha "$review_sha" \
        --review-tree "$review_tree"); then
    printf '%s\n' 'error: valid admission was not ingested' >&2
    exit 1
fi
/usr/bin/rm -rf -- "$artifacts"
/usr/bin/rm -f -- "$receipt"
verified=$(cargo run -p marketplace-tool --locked --quiet -- \
    verify-admission --store "$store" --review-tree "$review_tree")
if test "$ingested" != "$review_tree 1" \
        || test "$verified" != "$review_tree 1"; then
    printf '%s\n' 'error: durable admission output is invalid' >&2
    exit 1
fi

printf '%s\n' 'Marketplace durable admission store smoke tests passed'
