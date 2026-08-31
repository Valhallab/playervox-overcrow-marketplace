#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3; then
    printf '%s\n' \
        'usage: accept-candidate-revision.sh TRUST-SHA REVIEW-SHA CANDIDATE-SHA' >&2
    exit 2
fi
trust_sha=$1
review_sha=$2
candidate_sha=$3
for revision in "$trust_sha" "$review_sha" "$candidate_sha"; do
    case "$revision" in
        '' | *[!0-9a-f]*)
            printf '%s\n' 'error: accepted revision is invalid' >&2
            exit 1
            ;;
    esac
    if test "${#revision}" -ne 40; then
        printf '%s\n' 'error: accepted revision is invalid' >&2
        exit 1
    fi
done

logical_script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -L)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
if test "$logical_script_dir" != "$script_dir"; then
    printf '%s\n' 'error: accepted revision root is unsafe' >&2
    exit 1
fi
repo_root=$(/usr/bin/dirname -- "$script_dir")
case "$repo_root" in
    / | '') printf '%s\n' 'error: accepted revision root is unsafe' >&2; exit 1 ;;
esac

accepted_git() {
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

current_head=$(accepted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null) \
    || current_head=''
resolved_trust=$(accepted_git rev-parse --verify "$trust_sha^{commit}" 2>/dev/null) \
    || resolved_trust=''
resolved_review=$(accepted_git rev-parse --verify "$review_sha^{commit}" 2>/dev/null) \
    || resolved_review=''
resolved_candidate=$(accepted_git rev-parse --verify \
    "$candidate_sha^{commit}" 2>/dev/null) || resolved_candidate=''
protected_candidate=$(accepted_git show-ref --verify --hash \
    refs/remotes/origin/candidate 2>/dev/null) || protected_candidate=''
if accepted_git symbolic-ref -q refs/remotes/origin/candidate >/dev/null 2>&1 \
        || test "$current_head" != "$trust_sha" \
        || test "$resolved_trust" != "$trust_sha" \
        || test "$resolved_review" != "$review_sha" \
        || test "$resolved_candidate" != "$candidate_sha" \
        || test "$protected_candidate" != "$candidate_sha" \
        || test "$candidate_sha" = "$trust_sha"; then
    printf '%s\n' 'error: protected candidate revision is unavailable' >&2
    exit 1
fi

status_file=$(/usr/bin/mktemp /tmp/marketplace-accepted-status.XXXXXXXXXX) || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$status_file"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
if ! accepted_git status --porcelain=v1 --untracked-files=all >"$status_file" \
        || test ! -f "$status_file" || test -L "$status_file"; then
    printf '%s\n' 'error: accepted revision checkout is not clean' >&2
    exit 1
fi
status_size=$(/usr/bin/stat -c '%s' "$status_file")
case "$status_size" in
    '' | *[!0-9]*)
        printf '%s\n' 'error: accepted revision checkout is not clean' >&2
        exit 1
        ;;
esac
if test "$status_size" -gt 1048576 || test -s "$status_file"; then
    printf '%s\n' 'error: accepted revision checkout is not clean' >&2
    exit 1
fi

review_tree=$(accepted_git rev-parse --verify "$review_sha^{tree}" 2>/dev/null) \
    || review_tree=''
candidate_tree=$(accepted_git rev-parse --verify \
    "$candidate_sha^{tree}" 2>/dev/null) || candidate_tree=''
expected_base=$(accepted_git merge-base "$trust_sha" "$candidate_sha" 2>/dev/null) \
    || expected_base=''
if test -z "$review_tree" || test "$candidate_tree" != "$review_tree" \
        || test "$expected_base" != "$trust_sha" \
        || ! accepted_git merge-base --is-ancestor "$trust_sha" "$candidate_sha"; then
    printf '%s\n' 'error: protected candidate does not match the reviewed tree' >&2
    exit 1
fi

review_gate="$script_dir/review-revision.sh"
if test ! -f "$review_gate" || test -L "$review_gate" \
        || test ! -x "$review_gate"; then
    printf '%s\n' 'error: accepted revision complete gate is unavailable' >&2
    exit 1
fi
sh "$review_gate" "$trust_sha" "$candidate_sha"
