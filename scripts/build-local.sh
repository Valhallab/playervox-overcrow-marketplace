#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: build-local.sh' >&2
    exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(dirname -- "$script_dir")
if test ! -f "$repo_root/Cargo.toml" || test -L "$repo_root"; then
    printf '%s\n' 'error: marketplace repository is unavailable or unsafe' >&2
    exit 1
fi

exec 9<"$repo_root"
if ! /usr/bin/flock -n 9; then
    printf '%s\n' 'error: a local marketplace build is already running' >&2
    exit 1
fi

stage=$(/usr/bin/mktemp -d "$repo_root/.build-local.XXXXXXXXXX") || exit 1
source_root="$stage/repository"
component_paths=''
next_public=""
previous_public=""
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if test -n "$previous_public" && test -d "$previous_public" && ! test -e "$repo_root/public"; then
        /usr/bin/mv -- "$previous_public" "$repo_root/public" || status=1
    fi
    if test -n "$next_public" && { test -e "$next_public" || test -L "$next_public"; }; then
        /usr/bin/rm -rf -- "$next_public"
    fi
    if test -n "$previous_public" && { test -e "$previous_public" || test -L "$previous_public"; }; then
        /usr/bin/rm -rf -- "$previous_public"
    fi
    # shellcheck disable=SC2086 # paths are created by stage_component and contain no whitespace
    for component_path in $component_paths; do
        /usr/bin/rm -f -- "$component_path"
    done
    /usr/bin/rm -rf -- "$stage"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

cd "$repo_root"
cargo build --release --target wasm32-wasip2 --locked \
    -p warframe-worldstate-provider \
    -p warframe-status-widget \
    -p warframe-fissures-widget \
    -p warframe-sortie-archon-widget \
    -p warframe-invasions-widget \
    -p warframe-market-widget

/usr/bin/install -d -m 0700 "$source_root"
/usr/bin/cp -R -- marketplace fixtures providers widgets site "$source_root/"
cd "$source_root"

stage_component() {
    source_path=$1
    built_name=$2
    component_path="$source_path/component.wasm"
    if test -e "$component_path" || test -L "$component_path"; then
        printf '%s\n' "error: refusing to replace existing source component: $component_path" >&2
        exit 1
    fi
    /usr/bin/install -m 0644 "$repo_root/target/wasm32-wasip2/release/$built_name.wasm" "$component_path"
    component_paths="$component_paths $component_path"
}

replace_component_digest() {
    manifest_path=$1
    digest=$2
    temporary="$stage/$(/usr/bin/basename -- "$manifest_path").component"
    /usr/bin/awk -v digest="$digest" '
        /"path": "component.wasm"/ { component = 1 }
        component && /"sha256":/ {
            sub(/"[0-9a-f][0-9a-f]*"/, "\"" digest "\"")
            component = 0
        }
        { print }
    ' "$manifest_path" >"$temporary"
    /usr/bin/mv -f -- "$temporary" "$manifest_path"
}

replace_provider_digest() {
    manifest_path=$1
    digest=$2
    temporary="$stage/$(/usr/bin/basename -- "$manifest_path").provider"
    /usr/bin/awk -v digest="$digest" '
        /"id": "com.playervox.overcrow.warframe.worldstate"/ { provider = 1 }
        provider && /"sha256":/ {
            sub(/"[0-9a-f][0-9a-f]*"/, "\"" digest "\"")
            provider = 0
        }
        { print }
    ' "$manifest_path" >"$temporary"
    /usr/bin/mv -f -- "$temporary" "$manifest_path"
}

provider_package_digest() {
    provider_repository="$stage/provider-repository"
    /usr/bin/install -d -m 0700 \
        "$provider_repository/fixtures/keys" \
        "$provider_repository/marketplace" \
        "$provider_repository/providers"
    /usr/bin/cp -R -- providers/warframe-worldstate "$provider_repository/providers/"
    /usr/bin/install -m 0644 fixtures/keys/development-ed25519.key \
        "$provider_repository/fixtures/keys/development-ed25519.key"
    printf '%s\n' 1 >"$provider_repository/marketplace/development-sequence.txt"
    printf '%s\n' '[{"sourceDirectory":"providers/warframe-worldstate","status":"verified"}]' \
        >"$provider_repository/marketplace/targets.json"
    cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" --locked -- build \
        --repository "$provider_repository" \
        --generated-at 2026-08-27T00:00:00Z \
        --expires-at 2036-08-27T00:00:00Z \
        --development-key >/dev/null
    provider_package=$(/usr/bin/find \
        "$provider_repository/public/marketplace/v1/packages/com.playervox.overcrow.warframe.worldstate/1.0.0" \
        -type f -name '*.ocpkg' -print -quit)
    if test -z "$provider_package"; then
        printf '%s\n' 'error: provider package generation did not produce one object' >&2
        exit 1
    fi
    provider_digest=$(/usr/bin/basename -- "$provider_package" .ocpkg)
    printf '%s\n' "$provider_digest"
}

stage_component providers/warframe-worldstate warframe_worldstate_provider
stage_component widgets/warframe-status warframe_status_widget
stage_component widgets/warframe-fissures warframe_fissures_widget
stage_component widgets/warframe-sortie-archon warframe_sortie_archon_widget
stage_component widgets/warframe-invasions warframe_invasions_widget
stage_component widgets/warframe-market warframe_market_widget

replace_component_digest providers/warframe-worldstate/manifest.json \
    "$(/usr/bin/sha256sum providers/warframe-worldstate/component.wasm | /usr/bin/cut -d ' ' -f 1)"
provider_digest=$(provider_package_digest)
case "$provider_digest" in
    *[!0-9a-f]* | '')
        printf '%s\n' 'error: provider package digest is invalid' >&2
        exit 1
        ;;
esac
if test "${#provider_digest}" -ne 64; then
    printf '%s\n' 'error: provider package digest is invalid' >&2
    exit 1
fi

for widget in status fissures sortie-archon invasions market; do
    replace_component_digest "widgets/warframe-$widget/manifest.json" \
        "$(/usr/bin/sha256sum "widgets/warframe-$widget/component.wasm" | /usr/bin/cut -d ' ' -f 1)"
done
for widget in status fissures sortie-archon invasions; do
    replace_provider_digest "widgets/warframe-$widget/manifest.json" "$provider_digest"
done

cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" --locked -- build \
    --repository "$source_root" \
    --generated-at 2026-08-27T00:00:00Z \
    --expires-at 2036-08-27T00:00:00Z \
    --development-key
 cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" --locked -- verify public/marketplace/v1/catalog.json

require_identical_source() {
    relative=$1
    source="$source_root/$relative"
    destination="$repo_root/$relative"
    if test ! -f "$source" || test -L "$source" || test ! -f "$destination" || test -L "$destination"; then
        printf '%s\n' "error: unsafe tracked source path: $relative" >&2
        exit 1
    fi
    if ! /usr/bin/cmp -s -- "$source" "$destination"; then
        printf '%s\n' "error: generated tracked source differs: $relative" >&2
        exit 1
    fi
}
for relative in marketplace/development-catalog-state.json \
        providers/warframe-worldstate/manifest.json \
        widgets/warframe-status/manifest.json widgets/warframe-fissures/manifest.json \
        widgets/warframe-sortie-archon/manifest.json widgets/warframe-invasions/manifest.json \
        widgets/warframe-market/manifest.json; do
    require_identical_source "$relative"
done

for file in index.html app.js styles.css; do
    /usr/bin/install -m 0644 "site/$file" "public/$file"
    if test ! -f "public/$file" || test -L "public/$file"; then
        printf '%s\n' "error: generated site file is unsafe: $file" >&2
        exit 1
    fi
done

next_public="$repo_root/.public-next.$$"
previous_public="$repo_root/.public-previous.$$"
if test -e "$next_public" || test -L "$next_public"; then
    printf '%s\n' 'error: publication staging path already exists' >&2
    exit 1
fi
if test -e "$previous_public" || test -L "$previous_public"; then
    printf '%s\n' 'error: previous publication path already exists' >&2
    exit 1
fi
if test -e "$repo_root/public" && { test ! -d "$repo_root/public" || test -L "$repo_root/public"; }; then
    printf '%s\n' 'error: existing public path is unsafe' >&2
    exit 1
fi
/usr/bin/mv -- "$source_root/public" "$next_public"
if test -e "$repo_root/public"; then
    /usr/bin/mv -- "$repo_root/public" "$previous_public"
fi
if ! /usr/bin/mv -- "$next_public" "$repo_root/public"; then
    if test -e "$previous_public"; then /usr/bin/mv -- "$previous_public" "$repo_root/public"; fi
    exit 1
fi
if test -e "$previous_public"; then /usr/bin/rm -rf -- "$previous_public"; fi
cd "$repo_root"
