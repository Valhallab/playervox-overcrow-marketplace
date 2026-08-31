#!/bin/sh
set -eu
umask 077

if test "$#" -ne 9; then
    printf '%s\n' \
        'usage: ci-verify.sh REPOSITORY TRUST-SHA HEAD-SHA EVENT REPOSITORY-NAME BASE-REF HEAD-REPOSITORY HEAD-REF PRIVATE-PARENT' >&2
    exit 2
fi
repository=$1
trust_sha=$2
head_sha=$3
event_name=$4
repository_name=$5
base_ref=$6
head_repository=$7
head_ref=$8
private_parent=$9

case "$event_name" in
    pull_request | push) ;;
    *) printf '%s\n' 'error: CI trust metadata is invalid' >&2; exit 1 ;;
esac
for revision in "$trust_sha" "$head_sha"; do
    case "$revision" in
        *[!0-9a-f]* | '')
            printf '%s\n' 'error: CI trust metadata is invalid' >&2
            exit 1
            ;;
    esac
    if test "${#revision}" -ne 40; then
        printf '%s\n' 'error: CI trust metadata is invalid' >&2
        exit 1
    fi
done
case "$repository:$private_parent" in
    /*:/*) ;;
    *) printf '%s\n' 'error: CI trust metadata is invalid' >&2; exit 1 ;;
esac

invoking_uid=$(/usr/bin/id -u)
script_dir=$(CDPATH='' cd -- "$(/usr/bin/dirname -- "$0")" && pwd -P)
trusted_root=$(/usr/bin/dirname -- "$script_dir")
if test "$repository" = / || test "$trusted_root" = / || test "$private_parent" = / \
        || test ! -d "$repository" || test -L "$repository" \
        || test "$(CDPATH='' cd -- "$repository" && pwd -P)" != "$repository" \
        || test ! -d "$trusted_root" || test -L "$trusted_root" \
        || test ! -d "$private_parent" || test -L "$private_parent" \
        || test "$(CDPATH='' cd -- "$private_parent" && pwd -P)" != "$private_parent" \
        || test "$(/usr/bin/stat -c '%u:%a' "$private_parent")" \
            != "$invoking_uid:700"; then
    printf '%s\n' 'error: CI trust roots are unsafe' >&2
    exit 1
fi
for required in \
        scripts/materialize-git-snapshot.sh scripts/prepare-marketplace-tool.sh \
        scripts/ci-verify.sh scripts/sandbox-review-checks.sh \
        scripts/sandbox-component-build.sh scripts/sandbox-supervisor.c \
        tests/reject-published-change.sh tests/reject-trusted-change.sh \
        tests/check-community-change.mjs; do
    if test ! -f "$trusted_root/$required" || test -L "$trusted_root/$required"; then
        printf '%s\n' 'error: trusted CI driver is incomplete' >&2
        exit 1
    fi
done

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
            -c commit.gpgSign=false -c diff.external= -C "$repository" "$@"
}

for revision in "$trust_sha" "$head_sha"; do
    resolved=$(trusted_git rev-parse --verify "$revision^{commit}" 2>/dev/null) \
        || resolved=''
    if test "$resolved" != "$revision"; then
        printf '%s\n' 'error: CI trust metadata is invalid' >&2
        exit 1
    fi
done

work=$(/usr/bin/mktemp -d "$private_parent/verification.XXXXXXXXXX") || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
head_root="$work/head"
projection="$work/projection"
build_plan="$work/build-plan.tsv"
changed_paths="$work/changed-paths.nul"
/usr/bin/install -m 0600 /dev/null "$build_plan"
/usr/bin/install -m 0600 /dev/null "$changed_paths"

tool_work="$work/trusted-tool"
/usr/bin/install -d -m 0700 -- "$tool_work"
trusted_tool=$(sh "$trusted_root/scripts/prepare-marketplace-tool.sh" \
    "$trusted_root" "$tool_work")
sh "$trusted_root/scripts/materialize-git-snapshot.sh" --validated \
    "$repository" "$head_sha" "$head_root" "$trusted_tool"

if test "$event_name" = pull_request; then
    trusted_git diff --name-only -z --no-renames \
        "$trust_sha" "$head_sha" -- >"$changed_paths"
    if test "$(/usr/bin/stat -c '%u:%a:%h' "$changed_paths")" \
            != "$invoking_uid:600:1" \
            || test "$(/usr/bin/stat -c '%s' "$changed_paths")" -gt 262144; then
        printf '%s\n' 'error: pull-request path metadata is unsafe' >&2
        exit 1
    fi
    sh "$trusted_root/tests/reject-published-change.sh" \
        pull_request "$repository_name" "$base_ref" \
        "$head_repository" "$head_ref"
    /usr/bin/xargs -0 -r -- \
        sh "$trusted_root/tests/reject-published-change.sh" \
            pull_request "$repository_name" "$base_ref" \
            "$head_repository" "$head_ref" <"$changed_paths"
    /usr/bin/xargs -0 -r -- \
        sh "$trusted_root/tests/reject-trusted-change.sh" <"$changed_paths"
fi

if ! "$trusted_tool" build-plan --repository "$head_root" >"$build_plan" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$build_plan")" \
            != "$invoking_uid:600:1" \
        || test "$(/usr/bin/stat -c '%s' "$build_plan")" -gt 131072; then
    printf '%s\n' 'error: pull-request source admission failed' >&2
    exit 1
fi
if test "$event_name" = pull_request; then
    /usr/bin/node "$trusted_root/tests/check-community-change.mjs" \
        "$head_root" "$build_plan" "$changed_paths"
fi

# The candidate projection contains exact HEAD data, but all executable CI,
# policy, test, and marketplace-tool bytes come from the reviewed base.
/usr/bin/install -d -m 0700 -- "$projection"
/usr/bin/cp -a -- "$head_root/." "$projection/"
for trusted_path in .github scripts tests tools; do
    if test ! -d "$trusted_root/$trusted_path" \
            || test -L "$trusted_root/$trusted_path"; then
        printf '%s\n' 'error: trusted CI projection is unavailable' >&2
        exit 1
    fi
    /usr/bin/rm -rf -- "$projection/$trusted_path"
    /usr/bin/cp -a -- "$trusted_root/$trusted_path" \
        "$projection/$trusted_path"
done
if test -n "$(/usr/bin/find "$projection" -xdev ! -type d ! -type f -print -quit)" \
        || test -n "$(/usr/bin/find "$projection" -xdev ! -user "$invoking_uid" -print -quit)" \
        || test -n "$(/usr/bin/find "$projection" -xdev -perm /0022 -print -quit)" \
        || test "$(/usr/bin/find "$projection" -xdev -type f -printf . \
            | /usr/bin/wc -c)" -gt 1000; then
    printf '%s\n' 'error: trusted CI projection is unavailable' >&2
    exit 1
fi

projection_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/timeout --signal=KILL 30 \
        /usr/bin/prlimit --cpu=20 --as=1073741824 --nofile=128 \
            --fsize=33554432 -- \
        /usr/bin/git --no-replace-objects \
            -c core.fsmonitor=false -c core.hooksPath=/dev/null \
            -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
            -c commit.gpgSign=false -C "$projection" "$@"
}
projection_git init --quiet --initial-branch=review
projection_git add --force --all --
projection_git -c user.name='Marketplace CI' \
    -c user.email='marketplace-ci@invalid.example' \
    commit --quiet --no-gpg-sign -m 'reviewed CI projection'

stage_parent="$work/stage"
/usr/bin/install -d -m 0700 -- "$stage_parent"
sh "$projection/scripts/stage-catalog-repository.sh" --mode production \
    "$stage_parent/repository"

build_reviewed_catalog() {
    output=$1
    /usr/bin/install -d -m 0700 -- "$output"
    /usr/bin/cp -a -- "$stage_parent/repository/." "$output/"
    /usr/bin/rm -f -- "$output/marketplace/development-catalog-state.json"
    "$trusted_tool" build \
        --repository "$output" \
        --generated-at 2026-08-27T00:00:00Z \
        --expires-at 2036-08-27T00:00:00Z \
        --development-key
    "$trusted_tool" verify "$output/public/marketplace/v1/catalog.json"
    /usr/bin/cp -a -- "$projection/web/landing/." "$output/public/"
    /usr/bin/install -d -m 0700 -- "$output/public/marketplace"
    for file in index.html app.js styles.css; do
        /usr/bin/install -m 0600 -- "$projection/web/marketplace/$file" \
            "$output/public/marketplace/$file"
    done
    /usr/bin/install -m 0600 -- \
        "$projection/web/marketplace/policies/development.js" \
        "$output/public/marketplace/catalog-policy.js"
    if test -n "$(/usr/bin/find "$output/public" -xdev \
            ! -type d ! -type f -print -quit)" \
            || test -n "$(/usr/bin/find "$output/public" -xdev \
                ! -user "$invoking_uid" -print -quit)" \
            || test "$(/usr/bin/find "$output/public" -xdev -type f -printf . \
                | /usr/bin/wc -c)" -gt 1000; then
        printf '%s\n' 'error: reviewed catalog output is unsafe' >&2
        exit 1
    fi
}

first_build="$work/build-first"
second_build="$work/build-second"
build_reviewed_catalog "$first_build"
build_reviewed_catalog "$second_build"
/usr/bin/diff --recursive --no-dereference \
    "$first_build/public" "$second_build/public"
"$trusted_tool" verify "$first_build/public/marketplace/v1/catalog.json"
"$trusted_tool" verify "$second_build/public/marketplace/v1/catalog.json"

sh "$trusted_root/scripts/sandbox-review-checks.sh" workspace "$projection" "$first_build/public"
sh "$projection/scripts/sandbox-review-checks.sh" site \
    "$projection" "$first_build/public"

# These are reviewed base scripts. Community smoke remains a maintainer gate:
# it intentionally exercises local development paths and never runs on PR data.
sh "$projection/tests/ci-trust-boundary-smoke.sh"
sh "$projection/tests/sandbox-review-checks-smoke.sh"
sh "$projection/tests/sandbox-component-build-smoke.sh"
sh "$projection/tests/ci-policy-smoke.sh"
sh "$projection/scripts/check-policy.sh"
sh "$projection/tests/check-policy-smoke.sh"
/usr/bin/shellcheck "$projection"/scripts/*.sh "$projection"/tests/*.sh
/bin/sh -n "$projection"/scripts/*.sh "$projection"/tests/*.sh

printf '%s\n' 'Reviewed CI verification passed'
