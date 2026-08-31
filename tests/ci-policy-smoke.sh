#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: ci-policy-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
workflow="$repo_root/.github/workflows/ci.yml"
ci_driver="$repo_root/scripts/ci-verify.sh"
sandbox_driver="$repo_root/scripts/sandbox-review-checks.sh"
gate="$repo_root/tests/reject-published-change.sh"
community_gate="$repo_root/tests/check-community-change.mjs"
community_smoke="$repo_root/tests/community-submission-smoke.sh"
trusted_gate="$repo_root/tests/reject-trusted-change.sh"
trust_smoke="$repo_root/tests/ci-trust-boundary-smoke.sh"
sandbox_review_smoke="$repo_root/tests/sandbox-review-checks-smoke.sh"
codeowners="$repo_root/.github/CODEOWNERS"

for owned_path in \
        '/tests/reject-published-change.sh @Valhallab' \
        '/tests/reject-trusted-change.sh @Valhallab' \
        '/tests/check-community-change.mjs @Valhallab' \
        '/tests/community-submission-smoke.sh @Valhallab' \
        '/tests/ci-trust-boundary-smoke.sh @Valhallab' \
        '/tests/sandbox-review-checks-smoke.sh @Valhallab' \
        '/tests/ci-policy-smoke.sh @Valhallab'; do
    if ! /usr/bin/grep -F -x -- "$owned_path" "$codeowners" >/dev/null; then
        printf '%s\n' 'error: CI policy gates are not explicitly owned' >&2
        exit 1
    fi
done

if ! test -x "$gate" || test -L "$gate" \
        || ! test -x "$trusted_gate" || test -L "$trusted_gate"; then
    printf '%s\n' 'error: published-change gate is missing or unsafe' >&2
    exit 1
fi
if ! test -f "$community_gate" || test -L "$community_gate" \
        || ! test -x "$community_smoke" || test -L "$community_smoke" \
        || ! test -x "$trust_smoke" || test -L "$trust_smoke" \
        || ! test -x "$sandbox_review_smoke" || test -L "$sandbox_review_smoke"; then
    printf '%s\n' 'error: community-change policy tests are missing or unsafe' >&2
    exit 1
fi

expect_accept() {
    if ! "$gate" "$@"; then
        printf '%s\n' 'error: published-change gate rejected an allowed fixture' >&2
        exit 1
    fi
}

expect_reject() {
    if "$gate" "$@" >/dev/null 2>&1; then
        printf '%s\n' 'error: published-change gate accepted a forbidden fixture' >&2
        exit 1
    fi
}

repository=Valhallab/playervox-overcrow-marketplace
expect_accept pull_request "$repository" candidate creator/widget-fix \
    creator/widget-fork community/creator/widget/src/lib.rs \
    published-not/marketplace/v1/catalog.json
expect_reject pull_request "$repository" candidate creator/widget-fix \
    creator/widget-fork published/marketplace/v1/catalog.json
expect_accept pull_request "$repository" master "$repository" release/2026-09-01 \
    published/index.html published/marketplace/v1/catalog.json
expect_reject pull_request "$repository" master creator/widget-fork \
    release/2026-09-01 published/marketplace/v1/catalog.json
expect_reject pull_request "$repository" master "$repository" feature/not-a-release \
    published/marketplace/v1/catalog.json
expect_reject pull_request "$repository" master "$repository" release/unsafe/name \
    published/marketplace/v1/catalog.json
expect_reject push "$repository" master "$repository" release/2026-09-01 \
    published/marketplace/v1/catalog.json
expect_reject pull_request '' candidate creator/widget-fix creator/widget-fork \
    community/creator/widget/src/lib.rs
expect_reject pull_request "$repository" staging creator/widget-fix \
    creator/widget-fork community/creator/widget/src/lib.rs
expect_reject pull_request "$repository" candidate creator/widget-fix \
    creator/widget-fork '../published/marketplace/v1/catalog.json'
ambiguous_path=$(printf 'community/creator/widget/src/lib.rs\npublished/catalog.json')
expect_reject pull_request "$repository" candidate creator/widget-fix \
    creator/widget-fork "$ambiguous_path"

if ! "$trusted_gate" community/creator/widget/src/lib.rs Cargo.toml web/marketplace/app.js; then
    printf '%s\n' 'error: trusted-path gate rejected submission data' >&2
    exit 1
fi
for trusted_path in \
        .github/workflows/ci.yml scripts/ci-verify.sh tests/ci-policy-smoke.sh \
        tools/marketplace-tool/src/main.rs; do
    if "$trusted_gate" "$trusted_path" >/dev/null 2>&1; then
        printf '%s\n' 'error: trusted-path gate accepted executable policy changes' >&2
        exit 1
    fi
done

require_line() {
    if ! /usr/bin/grep -F -- "$1" \
            "$workflow" "$ci_driver" "$sandbox_driver" >/dev/null; then
        printf '%s\n' "error: CI policy is missing: $1" >&2
        exit 1
    fi
}

require_line 'branches: [master, candidate]'
require_line 'contents: read'
require_line 'persist-credentials: false'
require_line 'rustup toolchain install 1.98.0'
require_line 'bubblewrap'
require_line 'shellcheck'
require_line 'tests/reject-published-change.sh'
require_line 'tests/reject-trusted-change.sh'
require_line 'scripts/materialize-git-snapshot.sh'
require_line 'scripts/ci-verify.sh'
require_line 'scripts/sandbox-review-checks.sh'
require_line 'show "$TRUST_SHA:scripts/materialize-git-snapshot.sh"'
require_line 'sh "$trusted_root/scripts/ci-verify.sh"'
require_line 'tests/sandbox-component-build-smoke.sh'
require_line 'stage-catalog-repository.sh" --mode production'
require_line '--mode production --trusted-tool "$trusted_tool"'
require_line 'sh "$trusted_root/tests/ci-trust-boundary-smoke.sh" --trusted-root "$trusted_root" --trusted-tool "$trusted_tool"'
require_line '/usr/bin/node --test /source/tests/landing/*.test.mjs'
require_line '/usr/bin/node /source/tests/site-runtime.test.js'
require_line 'sandbox-review-checks.sh" workspace'
require_line 'sh "$trusted_root/scripts/sandbox-review-checks.sh" workspace "$projection" "$first_build/public"'
require_line 'sandbox-review-checks.sh" site'
require_line 'diff --recursive --no-dereference'
require_line '--unshare-all --unshare-net'
require_line 'CARGO_NET_OFFLINE=true'

if /usr/bin/grep -Eq \
        '(^|[[:space:]])(cargo|node|sh)[[:space:]]+(fmt|clippy|test|tests/|scripts/)' \
        "$workflow"; then
    printf '%s\n' 'error: CI executes pull-request code outside the trusted driver' >&2
    exit 1
fi

if /usr/bin/grep -Eq \
        'pull_request_target|pull-requests: write|contents: write|id-token: write|secrets\.|github\.token|GITHUB_TOKEN|upload-artifact|deploy|[Cc]oolify|build-production\.sh|--private-key' \
        "$workflow" "$ci_driver" "$sandbox_driver"; then
    printf '%s\n' 'error: CI has publication authority' >&2
    exit 1
fi

if /usr/bin/grep -F 'community-submission-smoke.sh' "$ci_driver" >/dev/null \
        || /usr/bin/grep -F 'scripts/build-local.sh' "$ci_driver" >/dev/null; then
    printf '%s\n' 'error: CI runs a local development path on pull-request data' >&2
    exit 1
fi

printf '%s\n' 'CI policy smoke tests passed'
