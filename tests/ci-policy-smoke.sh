#!/bin/sh
# shellcheck disable=SC2016 # Assertions below intentionally match literal shell/YAML.
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: ci-policy-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
workflow="$repo_root/.github/workflows/ci.yml"
ci_driver="$repo_root/scripts/ci-verify.sh"
sandbox="$repo_root/scripts/sandbox-review-checks.sh"
publisher="$repo_root/scripts/build-production.sh"
review_gate="$repo_root/scripts/review-revision.sh"
accept_gate="$repo_root/scripts/accept-candidate-revision.sh"
codeowners="$repo_root/.github/CODEOWNERS"

for path in \
        "$workflow" "$ci_driver" "$sandbox" "$publisher" \
        "$review_gate" "$accept_gate" \
        "$repo_root/scripts/review-bundle.sh" \
        "$repo_root/tests/reject-published-change.sh" \
        "$repo_root/tests/reject-trusted-change.sh"; do
    if test ! -f "$path" || test -L "$path"; then
        printf '%s\n' 'error: required CI boundary is missing or unsafe' >&2
        exit 1
    fi
done

for path in "$ci_driver" "$sandbox" "$publisher" "$review_gate" \
        "$accept_gate" "$repo_root/scripts/review-bundle.sh"; do
    if test ! -x "$path"; then
        printf '%s\n' 'error: required CI boundary is not executable' >&2
        exit 1
    fi
done

for owned in \
        '/scripts/ @ypMrg' \
        '/tools/ @ypMrg' \
        '/.github/ @ypMrg' \
        '/published/ @ypMrg' \
        '/tests/ci-policy-smoke.sh @ypMrg' \
        '/tests/community-submission-smoke.sh @ypMrg' \
        '/tests/accept-candidate-revision-smoke.sh @ypMrg'; do
    /usr/bin/grep -F -x -- "$owned" "$codeowners" >/dev/null || {
        printf '%s\n' 'error: executable trust boundaries are not owned' >&2
        exit 1
    }
done

published_gate="$repo_root/tests/reject-published-change.sh"
trusted_gate="$repo_root/tests/reject-trusted-change.sh"
repository=Valhallab/playervox-overcrow-marketplace
"$published_gate" pull_request "$repository" candidate creator/topic \
    creator/fork community/creator/widget/src/lib.rs
if "$published_gate" pull_request "$repository" candidate creator/topic \
        creator/fork published/marketplace/v1/catalog.json >/dev/null 2>&1; then
    printf '%s\n' 'error: contributor admission can modify published bytes' >&2
    exit 1
fi
"$published_gate" pull_request "$repository" master "$repository" \
    release/2026-09-01 published/marketplace/v1/catalog.json
if "$published_gate" pull_request "$repository" master creator/topic \
        creator/fork published/marketplace/v1/catalog.json >/dev/null 2>&1; then
    printf '%s\n' 'error: fork admission can modify published bytes' >&2
    exit 1
fi
"$trusted_gate" community/creator/widget/src/lib.rs Cargo.toml \
    web/marketplace/app.js
if "$trusted_gate" scripts/ci-verify.sh >/dev/null 2>&1 \
        || "$trusted_gate" tools/marketplace-tool/src/main.rs >/dev/null 2>&1; then
    printf '%s\n' 'error: contributor admission can modify executable policy' >&2
    exit 1
fi

if test "$(/usr/bin/grep -F -c -- 'pull_request_target:' "$workflow")" -ne 1 \
        || test "$(/usr/bin/grep -F -c -- 'push:' "$workflow")" -ne 1 \
        || test "$(/usr/bin/grep -F -c -- 'branches: [master, candidate]' \
            "$workflow")" -ne 2 \
        || ! /usr/bin/grep -F -x -- 'permissions: {}' "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- 'persist-credentials: false' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -x -- '  cancel-in-progress: true' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- \
            'actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- 'hashFiles('"'"'Cargo.lock'"'"')' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- \
            'Reject invalid paths before toolchain setup' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- \
            'diff --quiet --no-ext-diff' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- \
            '"$BASE_SHA" "$HEAD_SHA" -- .github scripts tests tools' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- '"$private_root" admission' \
            "$workflow" >/dev/null; then
    printf '%s\n' 'error: hosted admission scope is not explicit and read-only' >&2
    exit 1
fi

preflight_line=$(/usr/bin/grep -n -m1 \
    'Reject invalid paths before toolchain setup' "$workflow" \
    | /usr/bin/cut -d : -f 1)
toolchain_line=$(/usr/bin/grep -n -m1 'Install pinned admission toolchain' \
    "$workflow" | /usr/bin/cut -d : -f 1)
if test -z "$preflight_line" || test -z "$toolchain_line" \
        || test "$preflight_line" -ge "$toolchain_line"; then
    printf '%s\n' 'error: trusted-path rejection does not precede toolchain setup' >&2
    exit 1
fi
if /usr/bin/grep -F -x -- '  pull_request:' "$workflow" >/dev/null \
        || /usr/bin/grep -E -- '^[[:space:]]+(ref|repository):' \
            "$workflow" >/dev/null \
        || /usr/bin/grep -E -- \
            'pull-requests: write|contents: write|id-token: write|secrets\.|upload-artifact|deploy|[Cc]oolify|build-production[.]sh|--private-key' \
            "$workflow" "$ci_driver" "$sandbox" >/dev/null; then
    printf '%s\n' 'error: hosted admission has candidate or publication authority' >&2
    exit 1
fi
if ! /usr/bin/grep -F -- \
        'show "$TRUST_SHA:scripts/materialize-git-snapshot.sh"' \
        "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- \
            'refs/pull/$PR_NUMBER/head:refs/overcrow-review/$REVIEW_SHA' \
            "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -- '--no-tags --no-write-fetch-head' \
            "$workflow" >/dev/null; then
    printf '%s\n' 'error: hosted admission is not bound to reviewed Git objects' >&2
    exit 1
fi

admission_line=$(/usr/bin/grep -n -m1 'Hosted static admission passed' \
    "$ci_driver" | /usr/bin/cut -d : -f 1)
stage_line=$(/usr/bin/grep -n -m1 \
    '^[[:space:]]*sh .*stage-catalog-repository[.]sh' \
    "$ci_driver" | /usr/bin/cut -d : -f 1)
if test -z "$admission_line" || test -z "$stage_line" \
        || test "$admission_line" -ge "$stage_line" \
        || ! /usr/bin/grep -F -- '--changed-paths "$changed_paths"' \
            "$ci_driver" >/dev/null \
        || ! /usr/bin/grep -F -- '--reuse-components-from "$base_bundle/repository"' \
            "$ci_driver" >/dev/null \
        || ! /usr/bin/grep -F -- '--build-plan "$build_plan"' \
            "$ci_driver" >/dev/null \
        || ! /usr/bin/grep -F -- '--output "$review_bundle"' \
            "$ci_driver" >/dev/null; then
    printf '%s\n' 'error: review does not fail fast or reuse accepted artifacts' >&2
    exit 1
fi
if test "$(/usr/bin/grep -F -c -- 'build_reviewed_catalog "$first_build"' \
            "$ci_driver")" -ne 1 \
        || /usr/bin/grep -F -- 'second_build=' "$ci_driver" >/dev/null \
        || /usr/bin/grep -F -- 'diff --recursive' "$ci_driver" >/dev/null \
        || /usr/bin/grep -E -- \
            'community-submission-smoke|build-local[.]sh|for .*tests/.*smoke' \
            "$ci_driver" >/dev/null; then
    printf '%s\n' 'error: review repeats catalog assembly or unrelated smoke suites' >&2
    exit 1
fi

if ! /usr/bin/grep -F -- 'CARGO_NET_OFFLINE=true' "$sandbox" >/dev/null \
        || ! /usr/bin/grep -F -- '/usr/bin/unshare --net /usr/bin/setpriv' \
            "$sandbox" >/dev/null \
        || ! /usr/bin/grep -F -- '--bounding-set=-all' "$sandbox" >/dev/null \
        || ! /usr/bin/grep -F -- 'test "$3" = true' "$sandbox" >/dev/null \
        || ! /usr/bin/grep -F -- 'done </review-plan.tsv' "$sandbox" >/dev/null \
        || ! /usr/bin/grep -F -- 'for api_version in 1 2' "$sandbox" >/dev/null \
        || /usr/bin/grep -F -- 'cargo clippy' "$sandbox" >/dev/null \
        || /usr/bin/grep -F -- 'rm -rf -- /build/target' "$sandbox" >/dev/null; then
    printf '%s\n' 'error: sandbox review is not bounded, offline, and targeted' >&2
    exit 1
fi

if /usr/bin/grep -F -- 'stage-catalog-repository.sh' "$publisher" >/dev/null \
        || test "$(/usr/bin/grep -F -c -- \
            'verify --bundle "$review_bundle"' "$publisher")" -ne 1 \
        || test "$(/usr/bin/grep -F -c -- \
            'verify-copy --bundle "$review_bundle"' "$publisher")" -ne 1 \
        || ! /usr/bin/grep -F -- '--review-sha "$candidate_revision"' \
            "$publisher" >/dev/null; then
    printf '%s\n' 'error: publisher rebuilds or does not bind reviewed bytes' >&2
    exit 1
fi
if /usr/bin/grep -E -- \
        'review-revision[.]sh|ci-verify[.]sh|cargo (test|build)|sandbox-review-checks' \
        "$accept_gate" >/dev/null \
        || ! /usr/bin/grep -F -- '--review-sha "$review_sha"' \
            "$accept_gate" >/dev/null \
        || ! /usr/bin/grep -F -- '--trust-sha "$trust_sha"' \
            "$accept_gate" >/dev/null; then
    printf '%s\n' 'error: accepted-tree promotion repeats review work' >&2
    exit 1
fi
if ! /usr/bin/grep -F -- 'ACCEPTED-BASE-BUNDLE' "$review_gate" >/dev/null \
        || ! /usr/bin/grep -F -- '"$private_root" full "$review_bundle"' \
            "$review_gate" >/dev/null; then
    printf '%s\n' 'error: maintainer review cannot consume accepted evidence' >&2
    exit 1
fi

printf '%s\n' 'CI policy smoke tests passed'
