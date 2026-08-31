#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: ci-trust-boundary-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
materializer="$repo_root/scripts/materialize-git-snapshot.sh"
if test ! -x "$materializer" || test -L "$materializer"; then
    printf '%s\n' 'error: trusted snapshot materializer is missing or unsafe' >&2
    exit 1
fi

install_current_stage_script() {
    stage_repository=$1
    /usr/bin/install -m 0700 -- "$repo_root/scripts/stage-catalog-repository.sh" \
        "$stage_repository/scripts/stage-catalog-repository.sh"
    /usr/bin/git -C "$stage_repository" add -- scripts/stage-catalog-repository.sh
    if ! /usr/bin/git -C "$stage_repository" diff --cached --quiet; then
        /usr/bin/git -C "$stage_repository" commit --quiet \
            -m 'trusted staging interface fixture'
    fi
}

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-ci-trust.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

repository="$scratch/repository"
/usr/bin/git clone --quiet --no-hardlinks -- "$repo_root" "$repository"
/usr/bin/git -C "$repository" config user.name 'Marketplace Tests'
/usr/bin/git -C "$repository" config user.email 'marketplace-tests@invalid.example'
base_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')

base_root="$scratch/base"
sh "$materializer" --bootstrap "$repository" "$base_sha" "$base_root"
tool_work="$scratch/tool"
/usr/bin/install -d -m 0700 -- "$tool_work"
trusted_tool=$(sh "$base_root/scripts/prepare-marketplace-tool.sh" \
    "$base_root" "$tool_work")

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
install_current_stage_script "$clean_stage_repository"
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
install_current_stage_script "$malicious_stage_repository"
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
