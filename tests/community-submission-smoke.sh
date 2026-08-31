#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: community-submission-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
gate="$repo_root/tests/check-community-change.mjs"
if test ! -f "$gate" || test -L "$gate"; then
    printf '%s\n' 'error: community-change gate is missing or unsafe' >&2
    exit 1
fi
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-community.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

marketplace_tool() {
    cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
        --locked --quiet -- "$@"
}

write_path() {
    printf '%s\000' "$1" >"$2"
}

expect_gate_reject() {
    if node "$gate" "$@" >/dev/null 2>&1; then
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
marketplace_tool build-plan --repository "$fixture" >"$accepted_plan"
node "$gate" "$fixture" "$accepted_plan" "$changed_paths"

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
unwired_plan="$scratch/unwired-plan.tsv"
marketplace_tool build-plan --repository "$fixture" >"$unwired_plan"
expect_gate_reject "$fixture" "$unwired_plan" "$changed_paths"
/usr/bin/cp -- "$accepted_targets" "$fixture/marketplace/targets.json"

write_path 'community/example/BadWidget/src/lib.rs' "$changed_paths"
expect_gate_reject "$fixture" "$accepted_plan" "$changed_paths"
malformed_path=$(printf 'community/example/hello-widget/src/lib.rs\nother')
write_path "$malformed_path" "$changed_paths"
expect_gate_reject "$fixture" "$accepted_plan" "$changed_paths"
write_path 'community/example/hello-widget/../other/src/lib.rs' "$changed_paths"
expect_gate_reject "$fixture" "$accepted_plan" "$changed_paths"

: >"$changed_paths"
index=0
while test "$index" -lt 513; do
    printf 'docs/file-%s\000' "$index" >>"$changed_paths"
    index=$((index + 1))
done
expect_gate_reject "$fixture" "$accepted_plan" "$changed_paths"

: >"$changed_paths"
index=0
while test "$index" -lt 101; do
    printf 'community/p%s/w%s/src/lib.rs\000' "$index" "$index" \
        >>"$changed_paths"
    index=$((index + 1))
done
expect_gate_reject "$fixture" "$accepted_plan" "$changed_paths"
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
/usr/bin/install -d -m 0700 "$deleted_fixture/community/example"
deleted_plan="$scratch/deleted-plan.tsv"
printf '%s\t%s\t%s\n' \
    warframe-worldstate-provider warframe_worldstate_provider \
    providers/warframe-worldstate >"$deleted_plan"
node "$gate" "$deleted_fixture" "$deleted_plan" "$changed_paths"
printf '%s\t%s\t%s\n' hello-widget hello_widget \
    community/example/hello-widget >>"$deleted_plan"
expect_gate_reject "$deleted_fixture" "$deleted_plan" "$changed_paths"
/usr/bin/install -d -m 0700 \
    "$deleted_fixture/community/example/hello-widget"
expect_gate_reject "$deleted_fixture" "$scratch/unwired-plan.tsv" "$changed_paths"

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

sh "$fixture/scripts/build-local.sh"
first_public="$scratch/public-first"
/usr/bin/cp -R -- "$fixture/public" "$first_public"
sh "$fixture/scripts/build-local.sh"
/usr/bin/diff --recursive --no-dereference "$first_public" "$fixture/public"
if test "$(/usr/bin/find "$fixture/public/marketplace/v1/packages/com.playervox.overcrow.example.hello" \
        -type f -name '*.ocpkg' -print | /usr/bin/wc -l)" -ne 1; then
    printf '%s\n' 'error: wired community fixture was not packaged' >&2
    exit 1
fi

printf '%s\n' 'Community submission smoke tests passed'
