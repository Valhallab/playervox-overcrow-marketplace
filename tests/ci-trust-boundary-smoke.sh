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

printf '%s\n' 'CI trust-boundary smoke tests passed'
