#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: catalog-stage-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-catalog-stage.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

artifacts="$scratch/artifacts"
store="$scratch/accepted"
output="$scratch/staged"
/usr/bin/install -d -m 0700 -- "$artifacts" "$store" "$output"

package_output=$(cargo run -p marketplace-tool --locked --quiet -- \
    package "$repo_root/fixtures/hello-web" "$artifacts/1.ocpkg")
package_digest=${package_output%% *}
package_bytes=$(/usr/bin/stat -c '%s' "$artifacts/1.ocpkg")
listing="$artifacts/1.listing.json"
/usr/bin/install -m 0600 -- "$repo_root/fixtures/hello-web/listing.json" \
    "$listing"
listing_digest=$(/usr/bin/sha256sum "$listing" | /usr/bin/cut -d ' ' -f 1)
listing_bytes=$(/usr/bin/stat -c '%s' "$listing")
trust_sha=1111111111111111111111111111111111111111
review_sha=2222222222222222222222222222222222222222
review_tree=3333333333333333333333333333333333333333
receipt="$scratch/admission.tsv"
printf 'admission\t2\t%s\t%s\t%s\nartifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t1.0.0\t%s\t%s\t%s\t%s\n' \
    "$trust_sha" "$review_sha" "$review_tree" "$package_digest" \
    "$package_bytes" "$listing_digest" "$listing_bytes" >"$receipt"

cargo run -p marketplace-tool --locked --quiet -- \
    ingest --receipt "$receipt" --artifacts "$artifacts" --store "$store" \
    --trust-sha "$trust_sha" --review-sha "$review_sha" \
    --review-tree "$review_tree" >/dev/null
/usr/bin/rm -rf -- "$artifacts"
/usr/bin/rm -f -- "$receipt"

staged=$(cargo run -p marketplace-tool --locked --quiet -- \
    stage-development-catalog --store "$store" \
    --review-tree "$review_tree" --output "$output" --sequence 1 \
    --generated-at 2026-09-04T12:00:00Z \
    --expires-at 2026-10-04T12:00:00Z \
    --signing-key "$repo_root/fixtures/keys/development-ed25519.key")
staged_package="$output/packages/com.playervox.overcrow.hello/1.0.0/$package_digest.ocpkg"
stored_package="$store/packages/com.playervox.overcrow.hello/1.0.0/$package_digest.ocpkg"
if test "$staged" != "1 $output/catalog.json" \
        || test ! -s "$output/catalog.json" \
        || ! /usr/bin/cmp -s -- "$stored_package" "$staged_package" \
        || test "$(/usr/bin/find "$output" -type f | /usr/bin/wc -l)" -ne 2; then
    printf '%s\n' 'error: staged development catalog is incomplete' >&2
    exit 1
fi

printf '%s\n' 'Marketplace development catalog staging smoke tests passed'
