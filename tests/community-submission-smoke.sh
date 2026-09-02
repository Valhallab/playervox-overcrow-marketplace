#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: community-submission-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-community.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

marketplace_tool() {
    cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
        --locked --quiet -- "$@"
}

community_plan() {
    repository=$1
    changed=$2
    marketplace_tool build-plan --repository "$repository" \
        --changed-paths "$changed"
}

write_path() {
    printf '%s\000' "$1" >"$2"
    /usr/bin/chmod 0600 "$2"
}

expect_gate_reject() {
    if community_plan "$@" >/dev/null 2>&1; then
        printf '%s\n' 'error: community-change gate accepted a forbidden fixture' >&2
        exit 1
    fi
}

expect_plan_reject() {
    if marketplace_tool build-plan --repository "$1" >/dev/null 2>&1; then
        printf '%s\n' 'error: catalog admission accepted a forbidden community fixture' >&2
        exit 1
    fi
}

fixture="$scratch/repository"
/usr/bin/install -d -m 0700 "$fixture"
for path in \
        .gitignore Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures \
        providers widgets sdk wit examples tools web scripts community; do
    /usr/bin/cp -R -- "$repo_root/$path" "$fixture/"
done
/usr/bin/install -d -m 0700 "$fixture/community/example"
/usr/bin/mv -- "$fixture/examples/hello-widget" \
    "$fixture/community/example/hello-widget"
/usr/bin/sed -i \
    's|"examples/hello-widget"|"community/example/hello-widget"|' \
    "$fixture/Cargo.toml"
/usr/bin/sed -i \
    's|../../sdk/rust/overcrow-widget-sdk|../../../sdk/rust/overcrow-widget-sdk|' \
    "$fixture/community/example/hello-widget/Cargo.toml"

accepted_targets="$scratch/accepted-targets.json"
printf '%s\n' \
    '[' \
    '  {' \
    '    "sourceDirectory": "providers/warframe-worldstate",' \
    '    "cargoPackage": "warframe-worldstate-provider",' \
    '    "componentArtifact": "warframe_worldstate_provider",' \
    '    "status": "verified"' \
    '  },' \
    '  {' \
    '    "sourceDirectory": "widgets/warframe-status",' \
    '    "cargoPackage": "warframe-status-widget",' \
    '    "componentArtifact": "warframe_status_widget",' \
    '    "status": "verified"' \
    '  },' \
    '  {' \
    '    "sourceDirectory": "community/example/hello-widget",' \
    '    "cargoPackage": "hello-widget",' \
    '    "componentArtifact": "hello_widget",' \
    '    "status": "verified"' \
    '  }' \
    ']' >"$accepted_targets"
/usr/bin/cp -- "$accepted_targets" "$fixture/marketplace/targets.json"

changed_paths="$scratch/changed-paths"
write_path 'community/example/hello-widget/src/lib.rs' "$changed_paths"
accepted_plan="$scratch/accepted-plan.tsv"
community_plan "$fixture" "$changed_paths" >"$accepted_plan"
test "$(/usr/bin/wc -l <"$accepted_plan")" -eq 1
/usr/bin/grep -F -x \
    'hello-widget	hello_widget	community/example/hello-widget	1' \
    "$accepted_plan" >/dev/null

write_path 'sdk/rust/overcrow-widget-sdk/src/lib.rs' "$changed_paths"
all_plan="$scratch/all-plan.tsv"
community_plan "$fixture" "$changed_paths" >"$all_plan"
test "$(/usr/bin/wc -l <"$all_plan")" -eq 3

write_path 'web/marketplace/app.js' "$changed_paths"
web_plan="$scratch/web-plan.tsv"
community_plan "$fixture" "$changed_paths" >"$web_plan"
test ! -s "$web_plan"

write_path 'Cargo.lock' "$changed_paths"
shared_plan="$scratch/shared-plan.tsv"
community_plan "$fixture" "$changed_paths" >"$shared_plan"
test "$(/usr/bin/wc -l <"$shared_plan")" -eq 3

write_path 'community/example/hello-widget/src/lib.rs' "$changed_paths"

nested_fixture="$scratch/nested-repository"
/usr/bin/cp -R -- "$fixture" "$nested_fixture"
/usr/bin/mv -- "$nested_fixture/widgets/warframe-status" \
    "$nested_fixture/community/example/hello-widget/nested"
/usr/bin/sed -i \
    's|"widgets/warframe-status"|"community/example/hello-widget/nested"|' \
    "$nested_fixture/Cargo.toml" "$nested_fixture/marketplace/targets.json"
/usr/bin/sed -i \
    -e 's|../../sdk/rust/overcrow-widget-sdk|../../../../sdk/rust/overcrow-widget-sdk|' \
    -e 's|../warframe-data|../../../../widgets/warframe-data|' \
    "$nested_fixture/community/example/hello-widget/nested/Cargo.toml"
expect_gate_reject "$nested_fixture" "$changed_paths"

printf '%s\n' '[]' >"$fixture/marketplace/targets.json"
write_path 'docs/readme.md' "$changed_paths"
expect_gate_reject "$fixture" "$changed_paths"
/usr/bin/cp -- "$accepted_targets" "$fixture/marketplace/targets.json"
write_path 'community/example/hello-widget/src/lib.rs' "$changed_paths"

accepted_workspace="$scratch/accepted-workspace.toml"
/usr/bin/cp -- "$fixture/Cargo.toml" "$accepted_workspace"
/usr/bin/sed -i '/"community\/example\/hello-widget",/d' \
    "$fixture/Cargo.toml"
expect_plan_reject "$fixture"
/usr/bin/cp -- "$accepted_workspace" "$fixture/Cargo.toml"

printf '%s\n' \
    '[' \
    '  {' \
    '    "sourceDirectory": "providers/warframe-worldstate",' \
    '    "cargoPackage": "warframe-worldstate-provider",' \
    '    "componentArtifact": "warframe_worldstate_provider",' \
    '    "status": "verified"' \
    '  }' \
    ']' >"$fixture/marketplace/targets.json"
unwired_targets="$scratch/unwired-targets.json"
/usr/bin/cp -- "$fixture/marketplace/targets.json" "$unwired_targets"
unwired_plan="$scratch/unwired-plan.tsv"
marketplace_tool build-plan --repository "$fixture" >"$unwired_plan"
expect_gate_reject "$fixture" "$changed_paths"
/usr/bin/cp -- "$accepted_targets" "$fixture/marketplace/targets.json"

write_path 'community/example/BadWidget/src/lib.rs' "$changed_paths"
expect_gate_reject "$fixture" "$changed_paths"
malformed_path=$(printf 'community/example/hello-widget/src/lib.rs\nother')
write_path "$malformed_path" "$changed_paths"
expect_gate_reject "$fixture" "$changed_paths"
write_path 'community/example/hello-widget/../other/src/lib.rs' "$changed_paths"
expect_gate_reject "$fixture" "$changed_paths"

: >"$changed_paths"
index=0
while test "$index" -lt 513; do
    printf 'docs/file-%s\000' "$index" >>"$changed_paths"
    index=$((index + 1))
done
expect_gate_reject "$fixture" "$changed_paths"

: >"$changed_paths"
index=0
while test "$index" -lt 101; do
    printf 'community/p%s/w%s/src/lib.rs\000' "$index" "$index" \
        >>"$changed_paths"
    index=$((index + 1))
done
expect_gate_reject "$fixture" "$changed_paths"
write_path 'community/example/hello-widget/src/lib.rs' "$changed_paths"

creator_manifest="$fixture/community/example/hello-widget/Cargo.toml"
accepted_manifest="$scratch/accepted-Cargo.toml"
/usr/bin/cp -- "$creator_manifest" "$accepted_manifest"
printf '%s\n' 'fn main() {}' \
    >"$fixture/community/example/hello-widget/build.rs"
expect_plan_reject "$fixture"
/usr/bin/rm -f -- "$fixture/community/example/hello-widget/build.rs"

/usr/bin/sed -i \
    '/^\[dependencies\]$/a bad = { git = "https://example.invalid/repository" }' \
    "$creator_manifest"
expect_plan_reject "$fixture"
/usr/bin/cp -- "$accepted_manifest" "$creator_manifest"
/usr/bin/sed -i \
    '/^\[dependencies\]$/a bad = { version = "1", registry = "private" }' \
    "$creator_manifest"
expect_plan_reject "$fixture"
/usr/bin/cp -- "$accepted_manifest" "$creator_manifest"

deleted_fixture="$scratch/deleted-repository"
/usr/bin/cp -R -- "$fixture" "$deleted_fixture"
/usr/bin/cp -- "$unwired_targets" "$deleted_fixture/marketplace/targets.json"
/usr/bin/sed -i '/"community\/example\/hello-widget",/d' \
    "$deleted_fixture/Cargo.toml"
/usr/bin/awk '
    BEGIN { block = ""; drop = 0 }
    /^\[\[package\]\]$/ {
        if (block != "" && !drop) printf "%s", block
        block = $0 ORS
        drop = 0
        next
    }
    {
        block = block $0 ORS
        if ($0 == "name = \"hello-widget\"") drop = 1
    }
    END { if (block != "" && !drop) printf "%s", block }
' "$deleted_fixture/Cargo.lock" >"$deleted_fixture/Cargo.lock.next"
/usr/bin/mv -- "$deleted_fixture/Cargo.lock.next" "$deleted_fixture/Cargo.lock"
/usr/bin/rm -rf -- "$deleted_fixture/community/example/hello-widget"
printf 'community/example/hello-widget/src/lib.rs\000Cargo.toml\000Cargo.lock\000marketplace/targets.json\000' \
    >"$changed_paths"
/usr/bin/chmod 0600 "$changed_paths"
deleted_plan="$scratch/deleted-plan.tsv"
if ! community_plan "$deleted_fixture" "$changed_paths" >"$deleted_plan"; then
    printf '%s\n' 'error: community-change gate rejected a clean deletion' >&2
    exit 1
fi
test "$(/usr/bin/wc -l <"$deleted_plan")" -eq 1
tab=$(printf '\t')
/usr/bin/grep -F -x \
    "warframe-worldstate-provider${tab}warframe_worldstate_provider${tab}providers/warframe-worldstate${tab}1" \
    "$deleted_plan" >/dev/null
/usr/bin/cp -- "$accepted_targets" "$deleted_fixture/marketplace/targets.json"
expect_plan_reject "$deleted_fixture"

/usr/bin/git init --quiet "$fixture"
/usr/bin/git -C "$fixture" config user.name 'Marketplace Tests'
/usr/bin/git -C "$fixture" config user.email 'marketplace-tests@invalid.example'
/usr/bin/git -C "$fixture" add --all
/usr/bin/git -C "$fixture" commit --quiet -m 'reviewed community fixture'

production_stage="$scratch/production-stage"
sh "$fixture/scripts/stage-catalog-repository.sh" \
    --mode production "$production_stage"
for source in providers/warframe-worldstate community/example/hello-widget; do
    test -f "$production_stage/$source/component.wasm" \
        && test ! -L "$production_stage/$source/component.wasm"
    marketplace_tool inspect-component \
        "$production_stage/$source/component.wasm"
done

widget_source="$fixture/community/example/hello-widget/src/lib.rs"
/usr/bin/sed -i 's/Hello from OverCrow!/Hello again from OverCrow!/' \
    "$widget_source"
/usr/bin/git -C "$fixture" add community/example/hello-widget/src/lib.rs
/usr/bin/git -C "$fixture" commit --quiet -m 'updated community widget'
write_path 'community/example/hello-widget/src/lib.rs' "$changed_paths"
incremental_plan="$scratch/incremental-plan.tsv"
community_plan "$fixture" "$changed_paths" >"$incremental_plan"
/usr/bin/chmod 0600 "$incremental_plan"
test "$(/usr/bin/wc -l <"$incremental_plan")" -eq 1

incremental_stage="$scratch/incremental-stage"
sh "$fixture/scripts/stage-catalog-repository.sh" --mode production \
    --reuse-components-from "$production_stage" \
    --build-plan "$incremental_plan" "$incremental_stage"
/usr/bin/cmp -- "$production_stage/providers/warframe-worldstate/component.wasm" \
    "$incremental_stage/providers/warframe-worldstate/component.wasm"
if /usr/bin/cmp --silent \
        "$production_stage/community/example/hello-widget/component.wasm" \
        "$incremental_stage/community/example/hello-widget/component.wasm"; then
    printf '%s\n' 'error: changed community widget reused its old component' >&2
    exit 1
fi
marketplace_tool inspect-component \
    "$incremental_stage/community/example/hello-widget/component.wasm"

printf '%s\n' 'Community submission smoke tests passed'
