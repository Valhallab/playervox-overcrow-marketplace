#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: review-bundle-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
bundle_tool="$repo_root/scripts/review-bundle.sh"
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-review-bundle.XXXXXXXXXX)
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$scratch"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

source_root="$scratch/source"
bundle="$scratch/accepted.bundle"
/usr/bin/install -d -m 0700 -- "$source_root/widgets/example"
printf '%s\n' '{"id":"com.example.widget"}' \
    >"$source_root/widgets/example/manifest.json"
printf '\000asmreviewed' >"$source_root/widgets/example/component.wasm"
/usr/bin/chmod 0600 "$source_root/widgets/example/manifest.json"
/usr/bin/chmod 0644 "$source_root/widgets/example/component.wasm"
trust_sha=1111111111111111111111111111111111111111
review_sha=2222222222222222222222222222222222222222
review_tree=3333333333333333333333333333333333333333

sh "$bundle_tool" create --source "$source_root" --output "$bundle" \
    --trust-sha "$trust_sha" --review-sha "$review_sha" \
    --review-tree "$review_tree"
sh "$bundle_tool" verify --bundle "$bundle" --review-sha "$review_sha" \
    --review-tree "$review_tree"
sh "$bundle_tool" verify --bundle "$bundle" --review-sha "$review_sha" \
    --review-tree "$review_tree" --trust-sha "$trust_sha"
test "$(/usr/bin/stat -c '%a' "$bundle")" = 700
test "$(/usr/bin/stat -c '%a' "$bundle/receipt")" = 600
test "$(/usr/bin/stat -c '%a' "$bundle/ledger.tsv")" = 600
/usr/bin/cmp "$source_root/widgets/example/component.wasm" \
    "$bundle/repository/widgets/example/component.wasm"

copied_root="$scratch/copied-repository"
/usr/bin/cp -a -- "$bundle/repository" "$copied_root"
sh "$bundle_tool" verify-copy --bundle "$bundle" --copy "$copied_root" \
    --review-sha "$review_sha" --review-tree "$review_tree"
printf 'copy-drift' >>"$copied_root/widgets/example/component.wasm"
if sh "$bundle_tool" verify-copy --bundle "$bundle" --copy "$copied_root" \
        --review-sha "$review_sha" --review-tree "$review_tree" \
        >/dev/null 2>&1; then
    printf '%s\n' 'error: modified reviewed copy was accepted' >&2
    exit 1
fi

if sh "$bundle_tool" verify --bundle "$bundle" --review-sha "$review_sha" \
        --review-tree "$review_tree" \
        --trust-sha 4444444444444444444444444444444444444444 \
        >/dev/null 2>&1; then
    printf '%s\n' 'error: review bundle accepted the wrong trusted base' >&2
    exit 1
fi

if sh "$bundle_tool" create --source "$source_root" --output "$bundle" \
        --trust-sha "$trust_sha" --review-sha "$review_sha" \
        --review-tree "$review_tree" >/dev/null 2>&1; then
    printf '%s\n' 'error: existing review bundle was replaced' >&2
    exit 1
fi

printf 'changed' >>"$bundle/repository/widgets/example/component.wasm"
if sh "$bundle_tool" verify --bundle "$bundle" \
        --review-sha "$review_sha" \
        --review-tree "$review_tree" >/dev/null 2>&1; then
    printf '%s\n' 'error: modified reviewed component was accepted' >&2
    exit 1
fi
printf '\000asmreviewed' >"$bundle/repository/widgets/example/component.wasm"
/usr/bin/chmod 0644 "$bundle/repository/widgets/example/component.wasm"

if sh "$bundle_tool" verify --bundle "$bundle" \
        --review-sha 4444444444444444444444444444444444444444 \
        --review-tree "$review_tree" >/dev/null 2>&1; then
    printf '%s\n' 'error: review bundle accepted the wrong reviewed revision' >&2
    exit 1
fi

if sh "$bundle_tool" verify --bundle "$bundle" \
        --review-sha "$review_sha" \
        --review-tree 4444444444444444444444444444444444444444 \
        >/dev/null 2>&1; then
    printf '%s\n' 'error: review bundle accepted the wrong Git tree' >&2
    exit 1
fi

/usr/bin/mv "$bundle/repository/widgets/example/manifest.json" \
    "$scratch/manifest.json"
/usr/bin/ln -s "$scratch/manifest.json" \
    "$bundle/repository/widgets/example/manifest.json"
if sh "$bundle_tool" verify --bundle "$bundle" \
        --review-sha "$review_sha" \
        --review-tree "$review_tree" >/dev/null 2>&1; then
    printf '%s\n' 'error: review bundle accepted a symlink' >&2
    exit 1
fi

printf '%s\n' 'Review bundle smoke tests passed'
