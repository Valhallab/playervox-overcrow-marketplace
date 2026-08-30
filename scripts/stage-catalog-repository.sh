#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3 || test "$1" != "--mode"; then
    printf '%s\n' 'usage: stage-catalog-repository.sh --mode development|production OUTPUT-REPOSITORY' >&2
    exit 2
fi
mode=$2
output_repository=$3
case "$mode" in
    development | production) ;;
    *) printf '%s\n' 'error: invalid staging mode' >&2; exit 1 ;;
esac
case "$output_repository" in
    /*) ;;
    *) printf '%s\n' 'error: output repository must be absolute' >&2; exit 1 ;;
esac

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(/usr/bin/dirname -- "$script_dir")
output_parent=$(/usr/bin/dirname -- "$output_repository")
if test "$output_repository" = / || test -e "$output_repository" || test -L "$output_repository" \
        || test ! -d "$output_parent" || test -L "$output_parent" \
        || test "$(CDPATH='' cd -- "$output_parent" && pwd -P)" != "$output_parent" \
        || test "$(/usr/bin/stat -c '%u' "$output_parent")" != "$(/usr/bin/id -u)" \
        || /usr/bin/find "$output_parent" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: unsafe staging destination' >&2
    exit 1
fi

work=$(/usr/bin/mktemp -d "${output_repository}.build.XXXXXXXXXX") || exit 1
source_root="$work/repository"
target_root="$work/component-output"
provider_root="$work/provider-repository"
plan="$work/build-plan.tsv"
snapshot_archive="$work/reviewed-tree.tar"
production_revision=''
production_tree=''
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

marketplace_tool() {
    cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
        --locked --quiet -- "$@"
}

trusted_program() {
    program=$1
    test -f "$program" && test ! -L "$program" \
        && test "$(/usr/bin/stat -c '%u:%a' "$program")" = 0:755
}

trusted_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nofile=128 -- \
        /usr/bin/git --no-replace-objects -C "$repo_root" "$@"
}

git_candidate_is_clean() {
    revision=$1
    test "$(trusted_git rev-parse --show-toplevel 2>/dev/null)" = "$repo_root" \
        && test "$(trusted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null)" = "$revision" \
        && trusted_git diff --quiet --no-ext-diff "$revision" -- \
        && trusted_git diff --cached --quiet --no-ext-diff "$revision" -- \
        && trusted_git status --porcelain=v1 --untracked-files=all --ignored=no \
            >/dev/null 2>&1 \
        && ! trusted_git status --porcelain=v1 --untracked-files=all --ignored=no \
            2>/dev/null | /usr/bin/grep -q . \
        && trusted_git status --porcelain=v1 --untracked-files=all --ignored=no \
            >/dev/null 2>&1
}

/usr/bin/install -d -m 0700 "$source_root" "$target_root"
case "$mode" in
    development)
        marketplace_tool build-plan --repository "$repo_root" >"$plan"
        for path in Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures providers widgets sdk wit examples tools; do
            if test ! -e "$repo_root/$path" || test -L "$repo_root/$path"; then
                printf '%s\n' 'error: required catalog source is unavailable' >&2
                exit 1
            fi
            /usr/bin/cp -R -- "$repo_root/$path" "$source_root/"
        done
        ;;
    production)
        for program in /usr/bin/env /usr/bin/git /usr/bin/tar /usr/bin/timeout /usr/bin/prlimit; do
            if ! trusted_program "$program"; then
                printf '%s\n' 'error: trusted snapshot tool is unavailable' >&2
                exit 1
            fi
        done
        production_revision=$(
            trusted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null
        ) || {
            printf '%s\n' 'error: reviewed Git revision is unavailable' >&2
            exit 1
        }
        case "$production_revision" in
            '' | *[!0-9a-f]*)
                printf '%s\n' 'error: reviewed Git revision is invalid' >&2
                exit 1
                ;;
        esac
        if test "${#production_revision}" -ne 40 && test "${#production_revision}" -ne 64; then
            printf '%s\n' 'error: reviewed Git revision is invalid' >&2
            exit 1
        fi
        if ! git_candidate_is_clean "$production_revision"; then
            printf '%s\n' 'error: production candidate provenance changed' >&2
            exit 1
        fi
        production_tree=$(
            trusted_git rev-parse --verify "$production_revision^{tree}" 2>/dev/null
        ) || {
            printf '%s\n' 'error: reviewed Git tree is unavailable' >&2
            exit 1
        }
        snapshot_stats=$(
            marketplace_tool snapshot-plan \
                --repository "$repo_root" --revision "$production_revision"
        ) || {
            printf '%s\n' 'error: reviewed Git tree is unsafe' >&2
            exit 1
        }
        tab=$(printf '\t')
        IFS="$tab" read -r snapshot_entries snapshot_bytes <<EOF
$snapshot_stats
EOF
        case "$snapshot_entries:$snapshot_bytes" in
            *[!0-9:]* | :* | *:)
                printf '%s\n' 'error: reviewed Git tree bounds are invalid' >&2
                exit 1
                ;;
        esac
        if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
                GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
                GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
                /usr/bin/timeout --signal=KILL 30 \
                /usr/bin/prlimit --cpu=20 --as=536870912 --nproc=32 \
                    --nofile=128 --fsize=33554432 -- \
                /usr/bin/git --no-replace-objects -C "$repo_root" archive \
                    --format=tar --output="$snapshot_archive" "$production_revision" \
                    >/dev/null 2>&1; then
            printf '%s\n' 'error: reviewed Git tree materialization failed' >&2
            exit 1
        fi
        if test ! -f "$snapshot_archive" || test -L "$snapshot_archive" \
                || test "$(/usr/bin/stat -c '%u:%a:%h' "$snapshot_archive")" \
                    != "$(/usr/bin/id -u):600:1" \
                || test "$(/usr/bin/stat -c '%s' "$snapshot_archive")" -gt 33554432 \
                || ! /usr/bin/tar --extract --file="$snapshot_archive" \
                    --directory="$source_root" --no-same-owner --no-same-permissions \
                    --no-xattrs --no-acls --no-selinux; then
            printf '%s\n' 'error: reviewed Git tree extraction failed' >&2
            exit 1
        fi
        extracted_entries=$(
            /usr/bin/find "$source_root" -xdev -type f -printf . | /usr/bin/wc -c
        )
        extracted_bytes=$(
            /usr/bin/find "$source_root" -xdev -type f -printf '%s\n' \
                | /usr/bin/awk '{ total += $1 } END { printf "%.0f", total }'
        )
        if test "$extracted_entries" != "$snapshot_entries" \
                || test "$extracted_bytes" != "$snapshot_bytes" \
                || test -n "$(/usr/bin/find "$source_root" -xdev ! -type d ! -type f -print -quit)" \
                || test -n "$(/usr/bin/find "$source_root" -xdev ! -user "$(/usr/bin/id -u)" -print -quit)" \
                || test -n "$(/usr/bin/find "$source_root" -xdev -perm /0022 -print -quit)"; then
            printf '%s\n' 'error: reviewed Git tree extraction is unsafe' >&2
            exit 1
        fi
        marketplace_tool build-plan --repository "$source_root" >"$plan"
        ;;
esac

tab=$(printf '\t')
target_count=0
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    if test -z "$cargo_package" || test -z "$component_artifact" || test -z "$source_directory"; then
        printf '%s\n' 'error: invalid validated build plan' >&2
        exit 1
    fi
    component="$source_root/$source_directory/component.wasm"
    if test -e "$component" || test -L "$component"; then
        printf '%s\n' 'error: source component destination already exists' >&2
        exit 1
    fi
    target_count=$((target_count + 1))
done <"$plan"
if test "$target_count" -eq 0 || test "$target_count" -gt 500; then
    printf '%s\n' 'error: invalid validated build plan' >&2
    exit 1
fi

case "$mode" in
    development)
        set --
        while IFS="$tab" read -r cargo_package component_artifact source_directory; do
            set -- "$@" -p "$cargo_package"
        done <"$plan"
        RUSTFLAGS="--remap-path-prefix=$source_root=/usr/src/overcrow" \
            CARGO_INCREMENTAL=0 \
            cargo build --manifest-path "$source_root/Cargo.toml" \
            --release --target wasm32-wasip2 --target-dir "$target_root/target" \
            --locked --offline "$@"
        ;;
    production)
        /usr/bin/install -m 0600 -- "$plan" "$target_root/build-plan.tsv"
        sh "$source_root/scripts/sandbox-component-build.sh" "$source_root" "$target_root"
        ;;
esac

components_json="$work/components.json"
printf '%s' '[' >"$components_json"
separator=''
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    case "$mode" in
        development)
            built="$target_root/target/wasm32-wasip2/release/$component_artifact.wasm"
            ;;
        production)
            built="$target_root/artifacts/$component_artifact.wasm"
            ;;
    esac
    destination="$source_root/$source_directory/component.wasm"
    if test ! -f "$built" || test -L "$built" \
            || test -e "$destination" || test -L "$destination"; then
        printf '%s\n' 'error: component build output is missing or unsafe' >&2
        exit 1
    fi
    /usr/bin/install -m 0644 -- "$built" "$destination"
    marketplace_tool inspect-component "$destination"
    digest=$(/usr/bin/sha256sum "$destination" | /usr/bin/cut -d ' ' -f 1)
    printf '%s{"sourceDirectory":"%s","sha256":"%s"}' \
        "$separator" "$source_directory" "$digest" >>"$components_json"
    separator=,
done <"$plan"
printf '%s\n' ']' >>"$components_json"

# The first catalog has one reviewed provider. Fail closed if that reviewed
# identity disappears or another provider is introduced without extending the
# provider-first policy and its tests.
provider_source='providers/warframe-worldstate'
provider_package=''
provider_artifact=''
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    if test "$source_directory" = "$provider_source"; then
        provider_package=$cargo_package
        provider_artifact=$component_artifact
    fi
done <"$plan"
if test -z "$provider_package" || test -z "$provider_artifact"; then
    printf '%s\n' 'error: reviewed provider is absent from the build plan' >&2
    exit 1
fi

/usr/bin/cp -R -- "$source_root" "$provider_root"
/usr/bin/rm -f -- "$provider_root/marketplace/development-catalog-state.json"
printf '%s\n' \
    '[' \
    '  {' \
    "    \"sourceDirectory\": \"$provider_source\"," \
    "    \"cargoPackage\": \"$provider_package\"," \
    "    \"componentArtifact\": \"$provider_artifact\"," \
    '    "status": "verified"' \
    '  }' \
    ']' >"$provider_root/marketplace/targets.json"
provider_component_digest=$(
    /usr/bin/sha256sum "$provider_root/$provider_source/component.wasm" \
        | /usr/bin/cut -d ' ' -f 1
)
provider_bindings="$provider_root/.build-bindings.json"
printf '%s\n' \
    '{' \
    '  "schemaVersion": 1,' \
    '  "components": [' \
    "    {\"sourceDirectory\":\"$provider_source\",\"sha256\":\"$provider_component_digest\"}" \
    '  ],' \
    '  "providers": []' \
    '}' >"$provider_bindings"
/usr/bin/chmod 0600 "$provider_bindings"
marketplace_tool bind-build --repository "$provider_root" --bindings "$provider_bindings"
marketplace_tool build \
    --repository "$provider_root" \
    --generated-at 2026-08-27T00:00:00Z \
    --expires-at 2036-08-27T00:00:00Z \
    --development-key >/dev/null

provider_packages="$work/provider-packages"
/usr/bin/find "$provider_root/public/marketplace/v1/packages" \
    -type f -name '*.ocpkg' -print >"$provider_packages"
if test "$(/usr/bin/wc -l <"$provider_packages")" -ne 1; then
    printf '%s\n' 'error: provider-first packaging did not produce exactly one object' >&2
    exit 1
fi
provider_object=$(/usr/bin/cat "$provider_packages")
provider_relative=${provider_object#"$provider_root/public/marketplace/v1/packages/"}
provider_id=${provider_relative%%/*}
provider_remainder=${provider_relative#*/}
provider_version=${provider_remainder%%/*}
provider_file=${provider_remainder#*/}
provider_digest=${provider_file%.ocpkg}
if test "$provider_id/$provider_version/$provider_digest.ocpkg" != "$provider_relative"; then
    printf '%s\n' 'error: provider package path is invalid' >&2
    exit 1
fi

bindings="$source_root/.build-bindings.json"
printf '%s\n' '{' '  "schemaVersion": 1,' >"$bindings"
printf '%s' '  "components": ' >>"$bindings"
/usr/bin/cat "$components_json" >>"$bindings"
printf '%s\n' ',' >>"$bindings"
printf '%s\n' '  "providers": [' >>"$bindings"
printf '    {"id":"%s","version":"%s","sha256":"%s"}\n' \
    "$provider_id" "$provider_version" "$provider_digest" >>"$bindings"
printf '%s\n' '  ]' '}' >>"$bindings"
/usr/bin/chmod 0600 "$bindings"
marketplace_tool bind-build --repository "$source_root" --bindings "$bindings"

marketplace_tool build-plan --repository "$source_root" >/dev/null
if test "$mode" = production; then
    current_tree=$(
        trusted_git rev-parse --verify "$production_revision^{tree}" 2>/dev/null
    ) || current_tree=''
    if test "$current_tree" != "$production_tree" \
            || ! git_candidate_is_clean "$production_revision"; then
        printf '%s\n' 'error: production candidate provenance changed' >&2
        exit 1
    fi
fi
if test -e "$output_repository" || test -L "$output_repository"; then
    printf '%s\n' 'error: staging destination appeared during the build' >&2
    exit 1
fi
/usr/bin/mv -T -- "$source_root" "$output_repository"
