#!/bin/sh
set -eu
umask 077

usage() {
    printf '%s\n' \
        'usage: ci-trust-boundary-smoke.sh [--trusted-root ABSOLUTE-ROOT --trusted-tool ABSOLUTE-TOOL]' >&2
}

case "$#:${1:-}" in
    0:)
        mode=bootstrap
        trusted_root_argument=''
        trusted_tool_argument=''
        ;;
    4:--trusted-root)
        if test "$3" != --trusted-tool; then
            usage
            exit 2
        fi
        mode=injected
        trusted_root_argument=$2
        trusted_tool_argument=$4
        ;;
    *)
        usage
        exit 2
        ;;
esac

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-ci-trust.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

if test "$mode" = bootstrap; then
    materializer="$repo_root/scripts/materialize-git-snapshot.sh"
    if test ! -x "$materializer" || test -L "$materializer"; then
        printf '%s\n' \
            'error: trusted snapshot materializer is missing or unsafe' >&2
        exit 1
    fi
    repository="$scratch/bootstrap-repository"
    /usr/bin/git clone --quiet --no-hardlinks -- "$repo_root" "$repository"
    base_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
    base_root="$scratch/base"
    sh "$materializer" --bootstrap "$repository" "$base_sha" "$base_root"
    tool_work="$scratch/tool"
    /usr/bin/install -d -m 0700 -- "$tool_work"
    trusted_tool=$(sh "$base_root/scripts/prepare-marketplace-tool.sh" \
        "$base_root" "$tool_work")

    candidate_projection="$scratch/candidate-projection"
    /usr/bin/git clone --quiet --no-hardlinks -- "$repository" \
        "$candidate_projection"
    /usr/bin/git -C "$candidate_projection" checkout --quiet "$base_sha"
    /usr/bin/git -C "$candidate_projection" config user.name 'Marketplace Tests'
    /usr/bin/git -C "$candidate_projection" config user.email \
        'marketplace-tests@invalid.example'
    /usr/bin/install -m 0700 -- "$repo_root/tests/ci-trust-boundary-smoke.sh" \
        "$candidate_projection/tests/ci-trust-boundary-smoke.sh"
    candidate_manifest="$candidate_projection/Cargo.toml"
    candidate_lock="$candidate_projection/Cargo.lock"
    candidate_manifest_next="$scratch/candidate-Cargo.toml"
    candidate_lock_next="$scratch/candidate-Cargo.lock"
    /usr/bin/awk '
        /^[[:space:]]*"tools\/marketplace-tool",[[:space:]]*$/ { next }
        { print }
    ' "$candidate_manifest" >"$candidate_manifest_next"
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
            if ($0 == "name = \"marketplace-tool\"") drop = 1
        }
        END {
            if (block != "" && !drop) printf "%s", block
        }
    ' "$candidate_lock" >"$candidate_lock_next"
    if /usr/bin/grep -F '"tools/marketplace-tool"' \
            "$candidate_manifest_next" >/dev/null \
            || /usr/bin/grep -F -x 'name = "marketplace-tool"' \
                "$candidate_lock_next" >/dev/null; then
        printf '%s\n' 'error: candidate-shaped Cargo fixture is invalid' >&2
        exit 1
    fi
    /usr/bin/install -m 0600 -- \
        "$candidate_manifest_next" "$candidate_manifest"
    /usr/bin/install -m 0600 -- "$candidate_lock_next" "$candidate_lock"
    /usr/bin/git -C "$candidate_projection" add -- \
        Cargo.toml Cargo.lock tests/ci-trust-boundary-smoke.sh
    /usr/bin/git -C "$candidate_projection" commit --quiet \
        -m 'candidate-shaped root Cargo manifests'
    if ! "$trusted_tool" build-plan --repository "$candidate_projection" \
            >/dev/null; then
        printf '%s\n' 'error: candidate-shaped Cargo fixture was not admitted' >&2
        exit 1
    fi
    if ! sh "$candidate_projection/tests/ci-trust-boundary-smoke.sh" \
            --trusted-root "$base_root" --trusted-tool "$trusted_tool"; then
        printf '%s\n' \
            'error: later CI evidence rebuilt trust from candidate Cargo manifests' >&2
        exit 1
    fi
    exit 0
fi

base_root=$trusted_root_argument
trusted_tool=$trusted_tool_argument
materializer="$base_root/scripts/materialize-git-snapshot.sh"
tool_parent=$(/usr/bin/dirname -- "$trusted_tool")
invoking_uid=$(/usr/bin/id -u)
case "$base_root:$trusted_tool" in
    /*:/*) ;;
    *) printf '%s\n' 'error: injected CI trust is unavailable' >&2; exit 1 ;;
esac
if test "$base_root" = / || test ! -d "$base_root" || test -L "$base_root" \
        || test "$(CDPATH='' cd -- "$base_root" && pwd -P)" != "$base_root" \
        || test -e "$base_root/.git" || test -L "$base_root/.git" \
        || test "$(/usr/bin/stat -c '%u:%a' "$base_root")" \
            != "$invoking_uid:700" \
        || test -n "$(/usr/bin/find "$base_root" -xdev ! -type d ! -type f -print -quit)" \
        || test -n "$(/usr/bin/find "$base_root" -xdev ! -user "$invoking_uid" -print -quit)" \
        || test -n "$(/usr/bin/find "$base_root" -xdev -perm /0022 -print -quit)" \
        || test ! -x "$materializer" || test -L "$materializer" \
        || test ! -f "$trusted_tool" || test -L "$trusted_tool" \
        || test ! -d "$tool_parent" || test -L "$tool_parent" \
        || test "$(CDPATH='' cd -- "$tool_parent" && pwd -P)" != "$tool_parent" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$trusted_tool")" \
            != "$invoking_uid:700:1" \
        || test "$(/usr/bin/stat -c '%u:%a' "$tool_parent")" \
            != "$invoking_uid:700"; then
    printf '%s\n' 'error: injected CI trust is unavailable' >&2
    exit 1
fi

repository="$scratch/repository"
/usr/bin/install -d -m 0700 -- "$repository"
/usr/bin/cp -a -- "$base_root/." "$repository/"
/usr/bin/git -C "$repository" init --quiet --initial-branch=fixture
/usr/bin/git -C "$repository" config user.name 'Marketplace Tests'
/usr/bin/git -C "$repository" config user.email 'marketplace-tests@invalid.example'
/usr/bin/git -C "$repository" add --all --
/usr/bin/git -C "$repository" commit --quiet -m 'exact trusted base fixture'
base_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')

wrapper_marker="$scratch/rustc-wrapper-ran"
wrapper="$scratch/rustc-wrapper"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' ran >'$wrapper_marker'" \
    'exec "$@"' >"$wrapper"
/usr/bin/chmod 0700 "$wrapper"
/usr/bin/install -d -m 0700 -- "$repository/.cargo"
printf '%s\n' \
    '[build]' \
    "rustc-wrapper = \"$wrapper\"" >"$repository/.cargo/config.toml"
/usr/bin/git -C "$repository" add -- .cargo/config.toml
/usr/bin/git -C "$repository" commit --quiet -m 'malicious Cargo wrapper'
head_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')

head_root="$scratch/head"
sh "$materializer" --validated "$repository" "$head_sha" \
    "$head_root" "$trusted_tool"
if "$trusted_tool" build-plan --repository "$head_root" >/dev/null 2>&1; then
    printf '%s\n' 'error: trusted admission accepted a root Cargo wrapper' >&2
    exit 1
fi
if test -e "$wrapper_marker" || test -L "$wrapper_marker"; then
    printf '%s\n' 'error: trusted admission executed a root Cargo wrapper' >&2
    exit 1
fi

printf '%s\n' 'Cargo.toml export-ignore' \
    >"$repository/.gitattributes"
/usr/bin/git -C "$repository" add -- .gitattributes
/usr/bin/git -C "$repository" commit --quiet -m 'omit trusted CI driver'
export_ignored_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
if sh "$materializer" --validated "$repository" "$export_ignored_sha" \
        "$scratch/export-ignored" "$trusted_tool" >/dev/null 2>&1; then
    printf '%s\n' 'error: trusted materialization accepted omitted revision bytes' >&2
    exit 1
fi

clean_stage_repository="$scratch/clean-stage-repository"
/usr/bin/git clone --quiet --no-hardlinks -- "$repository" "$clean_stage_repository"
/usr/bin/git -C "$clean_stage_repository" checkout --quiet "$base_sha"
/usr/bin/git -C "$clean_stage_repository" config user.name 'Marketplace Tests'
/usr/bin/git -C "$clean_stage_repository" config user.email 'marketplace-tests@invalid.example'
clean_stage_output="$scratch/clean-trusted-stage"
if ! sh "$clean_stage_repository/scripts/stage-catalog-repository.sh" \
        --mode production --trusted-tool "$trusted_tool" "$clean_stage_output"; then
    printf '%s\n' 'error: trusted-tool staging rejected a clean fixture' >&2
    exit 1
fi
"$trusted_tool" build-plan --repository "$clean_stage_output" >/dev/null

stage_wrapper_marker="$scratch/stage-wrapper-ran"
stage_wrapper="$scratch/stage-rustc-wrapper"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' ran >'$stage_wrapper_marker'" \
    'exit 99' >"$stage_wrapper"
/usr/bin/chmod 0700 "$stage_wrapper"
malicious_stage_repository="$scratch/malicious-stage-repository"
/usr/bin/git clone --quiet --no-hardlinks -- "$repository" \
    "$malicious_stage_repository"
/usr/bin/git -C "$malicious_stage_repository" checkout --quiet "$base_sha"
/usr/bin/git -C "$malicious_stage_repository" config user.name 'Marketplace Tests'
/usr/bin/git -C "$malicious_stage_repository" config user.email 'marketplace-tests@invalid.example'
/usr/bin/install -d -m 0700 -- "$malicious_stage_repository/.cargo"
printf '%s\n' \
    '[build]' \
    "rustc-wrapper = \"$stage_wrapper\"" \
    >"$malicious_stage_repository/.cargo/config.toml"
/usr/bin/git -C "$malicious_stage_repository" add -- .cargo/config.toml
/usr/bin/git -C "$malicious_stage_repository" commit --quiet \
    -m 'malicious staging Cargo wrapper'
malicious_stage_output="$scratch/malicious-trusted-stage"
if sh "$malicious_stage_repository/scripts/stage-catalog-repository.sh" \
        --mode production --trusted-tool "$trusted_tool" \
        "$malicious_stage_output"; then
    printf '%s\n' 'error: trusted-tool staging accepted root Cargo config' >&2
    exit 1
fi
if test -e "$stage_wrapper_marker" || test -L "$stage_wrapper_marker"; then
    printf '%s\n' 'error: trusted-tool staging executed root Cargo config' >&2
    exit 1
fi
test ! -e "$malicious_stage_output" && test ! -L "$malicious_stage_output"

printf '%s\n' 'CI trust-boundary smoke tests passed'
