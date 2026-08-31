#!/bin/sh
set -eu
umask 077

if test "$#" -ne 2; then
    printf '%s\n' 'usage: review-revision.sh TRUST-SHA REVIEW-SHA' >&2
    exit 2
fi
trust_sha=$1
review_sha=$2
for revision in "$trust_sha" "$review_sha"; do
    case "$revision" in
        '' | *[!0-9a-f]*)
            printf '%s\n' 'error: review revision is invalid' >&2
            exit 1
            ;;
    esac
    if test "${#revision}" -ne 40; then
        printf '%s\n' 'error: review revision is invalid' >&2
        exit 1
    fi
done

logical_script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -L)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
if test "$logical_script_dir" != "$script_dir"; then
    printf '%s\n' 'error: maintainer review root is unsafe' >&2
    exit 1
fi
repo_root=$(/usr/bin/dirname -- "$script_dir")
case "$repo_root" in
    / | '') printf '%s\n' 'error: maintainer review root is unsafe' >&2; exit 1 ;;
esac

trusted_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/timeout --signal=KILL 15 \
        /usr/bin/prlimit --cpu=10 --as=1073741824 --nofile=128 \
            --fsize=33554432 -- \
        /usr/bin/git --no-replace-objects \
            -c core.fsmonitor=false -c core.hooksPath=/dev/null \
            -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
            -c commit.gpgSign=false -c diff.external= -C "$repo_root" "$@"
}

current_head=$(trusted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null) \
    || current_head=''
resolved_review=$(trusted_git rev-parse --verify "$review_sha^{commit}" 2>/dev/null) \
    || resolved_review=''
if test "$current_head" != "$trust_sha" || test "$resolved_review" != "$review_sha"; then
    printf '%s\n' 'error: review revision is unavailable' >&2
    exit 1
fi
status_file=$(/usr/bin/mktemp /tmp/marketplace-review-status.XXXXXXXXXX) || exit 1
private_root=''
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$status_file"
    if test -n "$private_root"; then
        /usr/bin/rm -rf -- "$private_root"
    fi
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
if ! trusted_git status --porcelain=v1 --untracked-files=all >"$status_file" \
        || test ! -f "$status_file" || test -L "$status_file"; then
    printf '%s\n' 'error: trusted review checkout is not clean' >&2
    exit 1
fi
status_size=$(/usr/bin/stat -c '%s' "$status_file")
case "$status_size" in
    '' | *[!0-9]*)
        printf '%s\n' 'error: trusted review checkout is not clean' >&2
        exit 1
        ;;
esac
if test "$status_size" -gt 1048576 || test -s "$status_file"; then
    printf '%s\n' 'error: trusted review checkout is not clean' >&2
    exit 1
fi

private_root=$(/usr/bin/mktemp -d /tmp/marketplace-review.XXXXXXXXXX) || exit 1
if test -L "$private_root" \
        || test "$(/usr/bin/stat -c '%u:%a' "$private_root")" \
            != "$(/usr/bin/id -u):700"; then
    printf '%s\n' 'error: private review workspace is unsafe' >&2
    exit 1
fi
bootstrap="$private_root/materialize-git-snapshot.sh"
trusted_git show "$trust_sha:scripts/materialize-git-snapshot.sh" >"$bootstrap"
/usr/bin/chmod 0700 "$bootstrap"
trusted_root="$private_root/trusted"
sh "$bootstrap" --bootstrap "$repo_root" "$trust_sha" "$trusted_root"

# Populate the shared Cargo cache only after the checkout and exact trusted
# snapshot have been validated. Candidate manifests are never passed to Cargo.
resolved_fetch=$(sh "$trusted_root/scripts/resolve-pinned-rust.sh" \
    --fetch "$trusted_root") || exit 1
tab=$(printf '\t')
IFS="$tab" read -r toolchain_root cargo_path rustc_path cargo_home <<EOF
$resolved_fetch
EOF
if test -z "$cargo_home"; then
    printf '%s\n' 'error: trusted dependency bootstrap is unavailable' >&2
    exit 1
fi
fetch_home="$private_root/fetch-home"
fetch_rustup_home="$private_root/fetch-rustup-home"
/usr/bin/install -d -m 0700 -- "$fetch_home" "$fetch_rustup_home"
(
    CDPATH='' cd -- "$trusted_root"
    /usr/bin/env -i \
        PATH="$toolchain_root/bin:/usr/bin:/bin" \
        HOME="$fetch_home" CARGO_HOME="$cargo_home" \
        RUSTUP_HOME="$fetch_rustup_home" RUSTC="$rustc_path" \
        CARGO_NET_RETRY=0 CARGO_HTTP_TIMEOUT=30 LC_ALL=C.UTF-8 LANG=C.UTF-8 \
        /usr/bin/timeout --signal=TERM --kill-after=5 300 \
        /usr/bin/prlimit --cpu=120 --as=4294967296 --nproc=4096 \
            --nofile=256 --fsize=268435456 -- \
        "$cargo_path" fetch --locked \
            --manifest-path tools/marketplace-tool/Cargo.toml
)

sh "$trusted_root/scripts/ci-verify.sh" \
    "$repo_root" "$trust_sha" "$review_sha" pull_request \
    Valhallab/playervox-overcrow-marketplace candidate \
    untrusted/review review \
    "$private_root" full
