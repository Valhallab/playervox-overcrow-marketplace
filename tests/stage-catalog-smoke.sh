#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: stage-catalog-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-stage.XXXXXXXXXX)
stage_pid=''
mutation_pid=''
cleanup() {
    if test -n "$mutation_pid"; then
        /usr/bin/kill "$mutation_pid" 2>/dev/null || true
        wait "$mutation_pid" 2>/dev/null || true
    fi
    if test -n "$stage_pid"; then
        /usr/bin/kill -CONT "$stage_pid" 2>/dev/null || true
        /usr/bin/kill "$stage_pid" 2>/dev/null || true
        wait "$stage_pid" 2>/dev/null || true
    fi
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

mutate_after_final_bind() {
    controlled_pid=$1
    race_output=$2
    relative=$3
    private_repository=''
    attempt=0
    while test -z "$private_repository"; do
        manifest=$(
            /usr/bin/find "$scratch" \
                -path "${race_output}.build.*/repository/widgets/warframe-status/manifest.json" \
                -print -quit 2>/dev/null || :
        )
        if test -n "$manifest"; then
            private_repository=${manifest%/widgets/warframe-status/manifest.json}
            original_inode=$(/usr/bin/stat -c '%i' "$manifest")
        fi
        attempt=$((attempt + 1))
        if test "$attempt" -ge 1200 || ! /usr/bin/kill -0 "$controlled_pid" 2>/dev/null; then
            return 1
        fi
        /usr/bin/sleep 0.05
    done
    attempt=0
    while test "$original_inode" = "$(/usr/bin/stat -c '%i' "$manifest" 2>/dev/null || :)"; do
        attempt=$((attempt + 1))
        if test "$attempt" -ge 12000 || ! /usr/bin/kill -0 "$controlled_pid" 2>/dev/null; then
            return 1
        fi
        /usr/bin/sleep 0.005
    done
    final_ledger="${private_repository%/repository}/final-file-ledger.tsv"
    if test -f "$final_ledger" && test ! -L "$final_ledger"; then
        attempt=0
        while test "$(/usr/bin/wc -l <"$final_ledger")" -ne 13; do
            attempt=$((attempt + 1))
            if test "$attempt" -ge 12000 \
                    || ! /usr/bin/kill -0 "$controlled_pid" 2>/dev/null; then
                return 1
            fi
            /usr/bin/sleep 0.005
        done
    fi
    /usr/bin/kill -STOP "$controlled_pid"
    /usr/bin/sleep 0.05
    mutation="$private_repository/$relative"
    mutation_size=$(/usr/bin/stat -c '%s' "$mutation")
    test "$mutation_size" -gt 0
    /usr/bin/dd if=/dev/zero of="$mutation" bs="$mutation_size" count=1 \
        conv=fsync 2>/dev/null
    /usr/bin/kill -CONT "$controlled_pid"
}

run_final_file_race() {
    label=$1
    relative=$2
    race_output=$3
    sh "$production_fixture/scripts/stage-catalog-repository.sh" \
        --mode production "$race_output" &
    stage_pid=$!
    mutate_after_final_bind "$stage_pid" "$race_output" "$relative" &
    mutation_pid=$!
    set +e
    wait "$stage_pid"
    stage_status=$?
    stage_pid=''
    wait "$mutation_pid"
    mutation_status=$?
    mutation_pid=''
    set -e
    if test "$mutation_status" -ne 0; then
        printf '%s\n' "error: $label race fixture did not reach the final bind" >&2
        exit 1
    fi
    if test "$stage_status" -eq 0; then
        printf '%s\n' "error: post-validation $label mutation was accepted" >&2
        final_race_failures=$((final_race_failures + 1))
    else
        test ! -e "$race_output" && test ! -L "$race_output"
    fi
    test "$production_manifest_before" = \
        "$(/usr/bin/sha256sum "$production_fixture/widgets/warframe-status/manifest.json")"
    test ! -e "$production_fixture/widgets/warframe-status/component.wasm" \
        && test ! -L "$production_fixture/widgets/warframe-status/component.wasm"
    test ! -e "$production_fixture/.build-bindings.json" \
        && test ! -L "$production_fixture/.build-bindings.json"
}

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
printf '%s\n' '$Format:%<(17)%h$' >"$production_fixture/export-subst.txt"
printf '%s\n' 'FILTER-RAW' >"$production_fixture/filter-target.txt"
/usr/bin/git -C "$production_fixture" add --all
/usr/bin/git -C "$production_fixture" commit --quiet -m 'reviewed production fixture'

git_hook_marker="$scratch/git-hook-ran"
fsmonitor_hook="$scratch/fsmonitor-hook"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' ran >'$git_hook_marker'" \
    'exit 1' >"$fsmonitor_hook"
/usr/bin/chmod 0700 "$fsmonitor_hook"
filter_hook="$scratch/filter-hook"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' ran >'$git_hook_marker'" \
    '/usr/bin/cat' >"$filter_hook"
/usr/bin/chmod 0700 "$filter_hook"
/usr/bin/git -C "$production_fixture" config core.fsmonitor "$fsmonitor_hook"
/usr/bin/git -C "$production_fixture" config filter.host-marker.clean "$filter_hook"
/usr/bin/git -C "$production_fixture" config filter.host-marker.smudge "$filter_hook"
printf '%s\n' \
    'export-subst.txt export-subst' \
    'filter-target.txt filter=host-marker' \
    >"$production_fixture/.git/info/attributes"

printf '%s\n' 'ignored environment secret' \
    >"$production_fixture/widgets/warframe-status/.env"
printf '%s\n' 'ignored signing key' \
    >"$production_fixture/widgets/warframe-status/ambient.key"
printf '%s\n' 'ignored temporary payload' \
    >"$production_fixture/widgets/warframe-status/ambient.tmp"
fake_path="$scratch/fake-path"
/usr/bin/install -d -m 0700 "$fake_path"
fake_cargo_marker="$scratch/fake-cargo-ran"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' ran >'$fake_cargo_marker'" \
    'exit 99' >"$fake_path/cargo"
/usr/bin/chmod 0700 "$fake_path/cargo"
production_stage="$scratch/production-stage"
PATH="$fake_path:/usr/bin:/bin" \
    sh "$production_fixture/scripts/stage-catalog-repository.sh" \
        --mode production "$production_stage"
test ! -e "$fake_cargo_marker" && test ! -L "$fake_cargo_marker"
test ! -e "$git_hook_marker" && test ! -L "$git_hook_marker"
/usr/bin/cmp -s -- "$production_fixture/export-subst.txt" \
    "$production_stage/export-subst.txt"
/usr/bin/cmp -s -- "$production_fixture/filter-target.txt" \
    "$production_stage/filter-target.txt"
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
while test -z "$(/usr/bin/find "$scratch" -path "${race_stage}.build.*/repository/Cargo.toml" -print -quit 2>/dev/null || :)"; do
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

source_race_stage="$scratch/source-race-production-stage"
sh "$production_fixture/scripts/stage-catalog-repository.sh" \
    --mode production "$source_race_stage" &
stage_pid=$!
attempt=0
source_race_file=''
while test -z "$source_race_file"; do
    source_race_file=$(
        /usr/bin/find "$scratch" \
            -path "${source_race_stage}.build.*/repository/web/landing/index.html" \
            -print -quit 2>/dev/null || :
    )
    attempt=$((attempt + 1))
    if test "$attempt" -ge 600; then
        printf '%s\n' 'error: source-integrity race did not reach its snapshot' >&2
        exit 1
    fi
    /usr/bin/sleep 0.05
done
source_race_size=$(/usr/bin/stat -c '%s' "$source_race_file")
/usr/bin/dd if=/dev/zero bs=1 count="$source_race_size" 2>/dev/null \
    | /usr/bin/tr '\000' X >"$source_race_file"
if wait "$stage_pid"; then
    stage_pid=''
    printf '%s\n' 'error: modified staged source was accepted' >&2
    exit 1
fi
stage_pid=''
test ! -e "$source_race_stage" && test ! -L "$source_race_stage"

production_manifest_before=$(
    /usr/bin/sha256sum "$production_fixture/widgets/warframe-status/manifest.json"
)
final_race_failures=0
run_final_file_race component \
    widgets/warframe-status/component.wasm \
    "$scratch/component-race-production-stage"
run_final_file_race manifest \
    widgets/warframe-status/manifest.json \
    "$scratch/manifest-race-production-stage"
run_final_file_race bindings \
    .build-bindings.json \
    "$scratch/bindings-race-production-stage"
if test "$final_race_failures" -ne 0; then
    exit 1
fi

printf '%s\n' 'Catalog staging smoke tests passed'
