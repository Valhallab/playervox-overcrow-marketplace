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
snapshot_plan_file="$work/snapshot-plan.tsv"
final_file_ledger="$work/final-file-ledger.tsv"
expected_final_paths="$work/expected-final-paths"
trusted_tool_binary=''
production_revision=''
production_tree=''
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
: >"$final_file_ledger"
/usr/bin/chmod 0600 -- "$final_file_ledger"

marketplace_tool() {
    if test "$mode" = production; then
        "$trusted_tool_binary" "$@"
    else
        cargo run --manifest-path "$repo_root/tools/marketplace-tool/Cargo.toml" \
            --locked --quiet -- "$@"
    fi
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
        /usr/bin/git --no-replace-objects \
            -c core.fsmonitor=false -c core.hooksPath=/dev/null \
            -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
            -c diff.external= -C "$repo_root" "$@"
}

git_candidate_is_clean() {
    revision=$1
    test "$(trusted_git rev-parse --show-toplevel 2>/dev/null)" = "$repo_root" \
        && test "$(trusted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null)" = "$revision" \
        && trusted_git diff-index --cached --quiet --no-ext-diff "$revision" -- \
        && ! trusted_git ls-files --others --exclude-per-directory=.gitignore \
            2>/dev/null | /usr/bin/grep -q . \
        && snapshot_matches_root "$repo_root"
}

final_ledger_contains_path() {
    relative=$1
    tab=$(printf '\t')
    while IFS="$tab" read -r ledger_path ledger_mode ledger_owner \
            ledger_links ledger_size ledger_digest ledger_extra; do
        if test "$ledger_path" = "$relative"; then
            return 0
        fi
    done <"$final_file_ledger"
    return 1
}

snapshot_matches_root() {
    root=$1
    allow_generated=${2:-no}
    tab=$(printf '\t')
    checked_entries=0
    checked_bytes=0
    while IFS="$tab" read -r expected_mode expected_size expected_oid relative; do
        case "$expected_mode:$expected_size:$expected_oid:$relative" in
            100644:* | 100755:*) ;;
            *) return 1 ;;
        esac
        file="$root/$relative"
        test -f "$file" && test ! -L "$file" || return 1
        is_generated=no
        if test "$allow_generated" = yes && final_ledger_contains_path "$relative"; then
            is_generated=yes
            test "$(/usr/bin/stat -c '%u:%h' "$file")" \
                = "$(/usr/bin/id -u):1" || return 1
        else
            test "$(/usr/bin/stat -c '%u:%h:%s' "$file")" \
                = "$(/usr/bin/id -u):1:$expected_size" || return 1
        fi
        if test "$expected_mode" = 100644; then
            test -z "$(/usr/bin/find "$file" -maxdepth 0 -perm /0111 -print -quit)" \
                || return 1
        else
            test -n "$(/usr/bin/find "$file" -maxdepth 0 -perm /0100 -print -quit)" \
                || return 1
        fi
        if test "$is_generated" = no; then
            actual_oid=$(trusted_git hash-object --no-filters -- "$file" 2>/dev/null) \
                || return 1
            test "$actual_oid" = "$expected_oid" || return 1
        fi
        checked_entries=$((checked_entries + 1))
        checked_bytes=$((checked_bytes + expected_size))
        test "$checked_entries" -le 1000 && test "$checked_bytes" -le 16777216 \
            || return 1
    done <"$snapshot_plan_file"
    test "$checked_entries" -gt 0
}

record_final_file() {
    relative=$1
    expected_mode=$2
    maximum_size=$3
    test "$mode" = production || return 0
    case "$relative" in
        '' | /* | */ | *[!A-Za-z0-9._/-]* | *//* | */../* | ../* | */.. | */./* | ./* | */.)
            return 1
            ;;
    esac
    test "${#relative}" -le 240 || return 1
    case "$expected_mode" in
        600 | 644) ;;
        *) return 1 ;;
    esac
    case "$maximum_size" in
        '' | *[!0-9]*) return 1 ;;
    esac
    ledger_entries=$(/usr/bin/wc -l <"$final_file_ledger") || return 1
    test "$ledger_entries" -lt 1001 || return 1
    if final_ledger_contains_path "$relative"; then
        return 1
    fi
    file="$source_root/$relative"
    test -f "$file" && test ! -L "$file" || return 1
    owner=$(/usr/bin/id -u) || return 1
    properties=$(/usr/bin/stat -c '%u:%a:%h:%s' "$file") || return 1
    size=$(/usr/bin/stat -c '%s' "$file") || return 1
    test "$properties" = "$owner:$expected_mode:1:$size" \
        && test "$size" -gt 0 && test "$size" -le "$maximum_size" \
        && test "$size" -le 8388608 || return 1
    digest=$(/usr/bin/sha256sum "$file" | /usr/bin/cut -d ' ' -f 1) || return 1
    case "$digest" in
        *[!0-9a-f]* | '') return 1 ;;
    esac
    test "${#digest}" -eq 64 || return 1
    printf '%s\t%s\t%s\t1\t%s\t%s\n' \
        "$relative" "$expected_mode" "$owner" "$size" "$digest" \
        >>"$final_file_ledger" || return 1
    test "$(/usr/bin/stat -c '%u:%a:%h' "$final_file_ledger")" \
        = "$owner:600:1" \
        && test "$(/usr/bin/stat -c '%s' "$final_file_ledger")" -le 524288
}

validate_final_file_ledger() {
    owner=$(/usr/bin/id -u) || return 1
    test -f "$final_file_ledger" && test ! -L "$final_file_ledger" \
        && test "$(/usr/bin/stat -c '%u:%a:%h' "$final_file_ledger")" \
            = "$owner:600:1" \
        && test "$(/usr/bin/stat -c '%s' "$final_file_ledger")" -le 524288 \
        || return 1
    ledger_digest_before=$(
        /usr/bin/sha256sum "$final_file_ledger" | /usr/bin/cut -d ' ' -f 1
    ) || return 1
    expected_entries=1
    final_ledger_contains_path .build-bindings.json || return 1
    tab=$(printf '\t')
    while IFS="$tab" read -r cargo_package component_artifact source_directory; do
        final_ledger_contains_path "$source_directory/component.wasm" || return 1
        final_ledger_contains_path "$source_directory/manifest.json" || return 1
        expected_entries=$((expected_entries + 2))
        test "$expected_entries" -le 1001 || return 1
    done <"$plan"
    test "$(/usr/bin/wc -l <"$final_file_ledger")" = "$expected_entries" \
        || return 1

    validated_entries=0
    validated_bytes=0
    validated_paths=''
    while IFS="$tab" read -r relative expected_mode expected_owner \
            expected_links expected_size expected_digest extra; do
        test -z "$extra" || return 1
        case "$relative:$expected_mode" in
            .build-bindings.json:600 | */manifest.json:644 | */component.wasm:644) ;;
            *) return 1 ;;
        esac
        case "$relative" in
            '' | /* | */ | *[!A-Za-z0-9._/-]* | *//* | */../* | ../* | */.. | */./* | ./* | */.)
                return 1
                ;;
        esac
        test "${#relative}" -le 240 \
            && test "$expected_owner" = "$owner" \
            && test "$expected_links" = 1 || return 1
        case "$expected_size" in
            '' | *[!0-9]*) return 1 ;;
        esac
        case "$expected_digest" in
            *[!0-9a-f]* | '') return 1 ;;
        esac
        test "${#expected_digest}" -eq 64 \
            && test "$expected_size" -gt 0 \
            && test "$expected_size" -le 8388608 || return 1
        case "
$validated_paths
" in
            *"
$relative
"*) return 1 ;;
        esac
        validated_paths="$validated_paths
$relative"
        file="$source_root/$relative"
        test -f "$file" && test ! -L "$file" \
            && test "$(/usr/bin/stat -c '%u:%a:%h:%s' "$file")" \
                = "$expected_owner:$expected_mode:$expected_links:$expected_size" \
            && test "$(/usr/bin/sha256sum "$file" | /usr/bin/cut -d ' ' -f 1)" \
                = "$expected_digest" || return 1
        validated_entries=$((validated_entries + 1))
        validated_bytes=$((validated_bytes + expected_size))
        test "$validated_entries" -le 1001 \
            && test "$validated_bytes" -le 268435456 || return 1
    done <"$final_file_ledger"
    test "$validated_entries" = "$expected_entries" \
        && test "$ledger_digest_before" = \
            "$(/usr/bin/sha256sum "$final_file_ledger" | /usr/bin/cut -d ' ' -f 1)"
}

materialize_snapshot() {
    tab=$(printf '\t')
    materialized_entries=0
    materialized_bytes=0
    while IFS="$tab" read -r expected_mode expected_size expected_oid relative; do
        case "$expected_mode:$expected_size:$expected_oid:$relative" in
            100644:* | 100755:*) ;;
            *) return 1 ;;
        esac
        destination="$source_root/$relative"
        parent=$(/usr/bin/dirname -- "$destination") || return 1
        /usr/bin/install -d -m 0700 -- "$parent" || return 1
        test ! -e "$destination" && test ! -L "$destination" || return 1
        if ! trusted_git cat-file blob "$expected_oid" >"$destination" 2>/dev/null; then
            return 1
        fi
        test "$(/usr/bin/stat -c '%s' "$destination")" = "$expected_size" \
            || return 1
        if test "$expected_mode" = 100755; then
            /usr/bin/chmod 0700 -- "$destination" || return 1
        else
            /usr/bin/chmod 0600 -- "$destination" || return 1
        fi
        materialized_entries=$((materialized_entries + 1))
        materialized_bytes=$((materialized_bytes + expected_size))
        test "$materialized_entries" -le 1000 \
            && test "$materialized_bytes" -le 16777216 || return 1
    done <"$snapshot_plan_file"
    test "$materialized_entries" -gt 0 \
        && test "$(/usr/bin/find "$source_root" -xdev -type f -printf . \
            | /usr/bin/wc -c)" = "$materialized_entries" \
        && test -z "$(/usr/bin/find "$source_root" -xdev ! -type d ! -type f -print -quit)" \
        && test -z "$(/usr/bin/find "$source_root" -xdev ! -user "$(/usr/bin/id -u)" -print -quit)" \
        && test -z "$(/usr/bin/find "$source_root" -xdev -perm /0022 -print -quit)" \
        && snapshot_matches_root "$source_root"
}

final_source_tree_is_expected() {
    test -z "$(/usr/bin/find "$source_root" -xdev ! -type d ! -type f -print -quit)" \
        && test -z "$(/usr/bin/find "$source_root" -xdev ! -user "$(/usr/bin/id -u)" -print -quit)" \
        && test -z "$(/usr/bin/find "$source_root" -xdev -perm /0022 -print -quit)" \
        || return 1
    actual_entries=0
    while IFS= read -r actual_path; do
        /usr/bin/grep -F -x -- "$actual_path" "$expected_final_paths" >/dev/null \
            || return 1
        actual_entries=$((actual_entries + 1))
    done <<EOF
$(/usr/bin/find "$source_root" -xdev -type f -printf '%P\n')
EOF
    test "$actual_entries" = "$(/usr/bin/wc -l <"$expected_final_paths")"
}

prepare_trusted_marketplace_tool() {
    tool_work="$work/trusted-tool"
    /usr/bin/install -d -m 0700 "$tool_work" || return 1
    trusted_tool_binary=$(sh "$script_dir/prepare-marketplace-tool.sh" \
        "$repo_root" "$tool_work") || return 1
}

/usr/bin/install -d -m 0700 "$source_root" "$target_root"
case "$mode" in
    development)
        marketplace_tool build-plan --repository "$repo_root" >"$plan"
        for path in \
                Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures \
                providers widgets sdk wit examples tools community; do
            if test ! -e "$repo_root/$path" || test -L "$repo_root/$path"; then
                printf '%s\n' 'error: required catalog source is unavailable' >&2
                exit 1
            fi
            /usr/bin/cp -R -- "$repo_root/$path" "$source_root/"
        done
        ;;
    production)
        for program in \
                /usr/bin/env /usr/bin/git /usr/bin/timeout /usr/bin/prlimit \
                /usr/bin/sha256sum; do
            if ! trusted_program "$program"; then
                printf '%s\n' 'error: trusted snapshot tool is unavailable' >&2
                exit 1
            fi
        done
        if ! prepare_trusted_marketplace_tool; then
            printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
            exit 1
        fi
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
        production_tree=$(
            trusted_git rev-parse --verify "$production_revision^{tree}" 2>/dev/null
        ) || {
            printf '%s\n' 'error: reviewed Git tree is unavailable' >&2
            exit 1
        }
        if ! marketplace_tool snapshot-plan \
                --repository "$repo_root" --revision "$production_revision" \
                >"$snapshot_plan_file"; then
            printf '%s\n' 'error: reviewed Git tree is unsafe' >&2
            exit 1
        fi
        /usr/bin/chmod 0600 -- "$snapshot_plan_file"
        if ! git_candidate_is_clean "$production_revision"; then
            printf '%s\n' 'error: production candidate provenance changed' >&2
            exit 1
        fi
        if ! materialize_snapshot; then
            printf '%s\n' 'error: reviewed Git tree materialization failed' >&2
            exit 1
        fi
        marketplace_tool build-plan --repository "$source_root" >"$plan"
        ;;
esac

tab=$(printf '\t')
target_count=0
if test "$mode" = production; then
    while IFS="$tab" read -r snapshot_mode snapshot_size snapshot_oid snapshot_relative; do
        printf '%s\n' "$snapshot_relative" >>"$expected_final_paths"
    done <"$snapshot_plan_file"
fi
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
    if test "$mode" = production; then
        printf '%s\n' "$source_directory/component.wasm" >>"$expected_final_paths"
    fi
    target_count=$((target_count + 1))
done <"$plan"
if test "$target_count" -eq 0 || test "$target_count" -gt 500; then
    printf '%s\n' 'error: invalid validated build plan' >&2
    exit 1
fi
if test "$mode" = production; then
    printf '%s\n' '.build-bindings.json' >>"$expected_final_paths"
    if test "$(/usr/bin/sort -u "$expected_final_paths" | /usr/bin/wc -l)" \
            != "$(/usr/bin/wc -l <"$expected_final_paths")"; then
        printf '%s\n' 'error: invalid validated build plan' >&2
        exit 1
    fi
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
    if ! record_final_file "$source_directory/component.wasm" 644 4194304; then
        printf '%s\n' 'error: generated source ledger failed' >&2
        exit 1
    fi
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
if test "$mode" = production; then
    if ! record_final_file .build-bindings.json 600 131072; then
        printf '%s\n' 'error: generated source ledger failed' >&2
        exit 1
    fi
    while IFS="$tab" read -r cargo_package component_artifact source_directory; do
        if ! record_final_file "$source_directory/manifest.json" 644 65536; then
            printf '%s\n' 'error: generated source ledger failed' >&2
            exit 1
        fi
    done <"$plan"
fi

marketplace_tool build-plan --repository "$source_root" >/dev/null
if test -e "$output_repository" || test -L "$output_repository"; then
    printf '%s\n' 'error: staging destination appeared during the build' >&2
    exit 1
fi
if test "$mode" = production; then
    current_tree=$(
        trusted_git rev-parse --verify "$production_revision^{tree}" 2>/dev/null
    ) || current_tree=''
    if test "$current_tree" != "$production_tree" \
            || ! git_candidate_is_clean "$production_revision" \
            || ! snapshot_matches_root "$source_root" yes \
            || ! final_source_tree_is_expected \
            || ! validate_final_file_ledger; then
        printf '%s\n' 'error: production candidate provenance changed' >&2
        exit 1
    fi
fi
/usr/bin/mv -T -- "$source_root" "$output_repository"
