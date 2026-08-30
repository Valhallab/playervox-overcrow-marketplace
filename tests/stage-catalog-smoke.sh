#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: stage-catalog-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-stage.XXXXXXXXXX)
stage_pid=''
cleanup() {
    if test -n "$stage_pid"; then
        /usr/bin/kill "$stage_pid" 2>/dev/null || true
        wait "$stage_pid" 2>/dev/null || true
    fi
    /usr/bin/rm -rf -- "$scratch"
}
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

production_fixture="$scratch/production-repository"
/usr/bin/install -d -m 0700 "$production_fixture"
for path in .gitignore Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures providers widgets sdk wit examples tools web scripts; do
    /usr/bin/cp -R -- "$repo_root/$path" "$production_fixture/"
done
/usr/bin/git init --quiet "$production_fixture"
/usr/bin/git -C "$production_fixture" config user.name 'Marketplace Tests'
/usr/bin/git -C "$production_fixture" config user.email 'marketplace-tests@invalid.example'
/usr/bin/git -C "$production_fixture" add --all
/usr/bin/git -C "$production_fixture" commit --quiet -m 'reviewed production fixture'

printf '%s\n' 'ignored environment secret' \
    >"$production_fixture/widgets/warframe-status/.env"
printf '%s\n' 'ignored signing key' \
    >"$production_fixture/widgets/warframe-status/ambient.key"
printf '%s\n' 'ignored temporary payload' \
    >"$production_fixture/widgets/warframe-status/ambient.tmp"
production_stage="$scratch/production-stage"
sh "$production_fixture/scripts/stage-catalog-repository.sh" \
    --mode production "$production_stage"
for relative in \
        widgets/warframe-status/.env \
        widgets/warframe-status/ambient.key \
        widgets/warframe-status/ambient.tmp; do
    if test -e "$production_stage/$relative" || test -L "$production_stage/$relative"; then
        printf '%s\n' 'error: ignored ambient bytes entered production source' >&2
        exit 1
    fi
done

printf '%s\n' 'dirty candidate bytes' \
    >>"$production_fixture/marketplace/targets.json"
if sh "$production_fixture/scripts/stage-catalog-repository.sh" \
        --mode production "$scratch/dirty-production-stage"; then
    printf '%s\n' 'error: dirty production provenance was accepted' >&2
    exit 1
fi
test ! -e "$scratch/dirty-production-stage" \
    && test ! -L "$scratch/dirty-production-stage"

/usr/bin/git --no-replace-objects -C "$production_fixture" \
    show HEAD:marketplace/targets.json \
    >"$production_fixture/marketplace/targets.json"
race_stage="$scratch/race-production-stage"
sh "$production_fixture/scripts/stage-catalog-repository.sh" \
    --mode production "$race_stage" &
stage_pid=$!
attempt=0
while test -z "$(/usr/bin/find "$scratch" -path "${race_stage}.build.*/repository/Cargo.toml" -print -quit)"; do
    attempt=$((attempt + 1))
    if test "$attempt" -ge 600; then
        printf '%s\n' 'error: production race fixture did not reach its snapshot' >&2
        exit 1
    fi
    /usr/bin/sleep 0.05
done
printf '%s\n' 'changed during production staging' >"$production_fixture/race-marker"
/usr/bin/git -C "$production_fixture" add race-marker
/usr/bin/git -C "$production_fixture" commit --quiet -m 'race production candidate'
if wait "$stage_pid"; then
    stage_pid=''
    printf '%s\n' 'error: changed production provenance was accepted' >&2
    exit 1
fi
stage_pid=''
test ! -e "$race_stage" && test ! -L "$race_stage"

printf '%s\n' 'Catalog staging smoke tests passed'
