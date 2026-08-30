#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: stage-catalog-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-stage.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM
fixture="$scratch/repository"
/usr/bin/install -d -m 0700 "$fixture"
for path in Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures providers widgets sdk wit examples tools web scripts; do
    /usr/bin/cp -R -- "$repo_root/$path" "$fixture/"
done

sentinel="$fixture/widgets/warframe-status/component.wasm"
printf '%s\n' 'creator-controlled component' >"$sentinel"
sentinel_before=$(/usr/bin/sha256sum "$sentinel")
if sh "$fixture/scripts/stage-catalog-repository.sh" \
        --mode development "$scratch/rejected-stage"; then
    printf '%s\n' 'error: staging replaced a creator component' >&2
    exit 1
fi
test "$sentinel_before" = "$(/usr/bin/sha256sum "$sentinel")"
test ! -e "$scratch/rejected-stage" && test ! -L "$scratch/rejected-stage"
/usr/bin/rm -f -- "$sentinel"

targets_before=$(/usr/bin/sha256sum "$fixture/marketplace/targets.json")
manifests_before="$scratch/manifests-before"
/usr/bin/find "$fixture/providers" "$fixture/widgets" -type f -name manifest.json -print0 \
    | /usr/bin/sort -z \
    | /usr/bin/xargs -0 /usr/bin/sha256sum >"$manifests_before"

staged="$scratch/staged-repository"
sh "$fixture/scripts/stage-catalog-repository.sh" --mode development "$staged"
test -d "$staged" && test ! -L "$staged"
test "$(/usr/bin/stat -c '%a' "$staged")" = 700
test -f "$staged/.build-bindings.json" && test ! -L "$staged/.build-bindings.json"
test "$(/usr/bin/stat -c '%a' "$staged/.build-bindings.json")" = 600
test "$targets_before" = "$(/usr/bin/sha256sum "$fixture/marketplace/targets.json")"
manifests_after="$scratch/manifests-after"
/usr/bin/find "$fixture/providers" "$fixture/widgets" -type f -name manifest.json -print0 \
    | /usr/bin/sort -z \
    | /usr/bin/xargs -0 /usr/bin/sha256sum >"$manifests_after"
/usr/bin/cmp -s -- "$manifests_before" "$manifests_after"

if ! env PATH="$PATH" cargo run --manifest-path "$fixture/tools/marketplace-tool/Cargo.toml" \
        --locked --quiet -- build-plan --repository "$staged" >"$scratch/build-plan"; then
    printf '%s\n' 'error: staged repository did not pass catalog admission' >&2
    exit 1
fi

# Re-read only validated output now that it exists.
component_count=0
tab=$(printf '\t')
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    test -n "$cargo_package"
    test -n "$component_artifact"
    component="$staged/$source_directory/component.wasm"
    test -f "$component" && test ! -L "$component"
    expected=$(/usr/bin/sha256sum "$component" | /usr/bin/cut -d ' ' -f 1)
    actual=$(
        /usr/bin/sed -n '/"component"[[:space:]]*:/,/}/p' \
            "$staged/$source_directory/manifest.json" \
            | /usr/bin/sed -n 's/.*"sha256"[[:space:]]*:[[:space:]]*"\([0-9a-f]*\)".*/\1/p'
    )
    test "$actual" = "$expected"
    component_count=$((component_count + 1))
done <"$scratch/build-plan"
test "$component_count" -eq 6

provider_digest=$(
    /usr/bin/sed -n '/"providers"[[:space:]]*:/,$p' "$staged/.build-bindings.json" \
        | /usr/bin/sed -n 's/.*"sha256"[[:space:]]*:[[:space:]]*"\([0-9a-f]*\)".*/\1/p'
)
test "${#provider_digest}" -eq 64
for widget in status fissures sortie-archon invasions; do
    /usr/bin/grep -F "\"sha256\": \"$provider_digest\"" \
        "$staged/widgets/warframe-$widget/manifest.json" >/dev/null
done

printf '%s\n' 'Catalog staging smoke tests passed'
