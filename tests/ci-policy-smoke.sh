#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: ci-policy-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
workflow="$repo_root/.github/workflows/ci.yml"
gate="$repo_root/tests/reject-published-change.sh"
community_gate="$repo_root/tests/check-community-change.mjs"
community_smoke="$repo_root/tests/community-submission-smoke.sh"
codeowners="$repo_root/.github/CODEOWNERS"

for owned_path in \
        '/tests/reject-published-change.sh @Valhallab' \
        '/tests/check-community-change.mjs @Valhallab' \
        '/tests/community-submission-smoke.sh @Valhallab' \
        '/tests/ci-policy-smoke.sh @Valhallab'; do
    if ! /usr/bin/grep -F -x -- "$owned_path" "$codeowners" >/dev/null; then
        printf '%s\n' 'error: CI policy gates are not explicitly owned' >&2
        exit 1
    fi
done

if ! test -x "$gate" || test -L "$gate"; then
    printf '%s\n' 'error: published-change gate is missing or unsafe' >&2
    exit 1
fi
if ! test -f "$community_gate" || test -L "$community_gate" \
        || ! test -x "$community_smoke" || test -L "$community_smoke"; then
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

require_line() {
    if ! /usr/bin/grep -F "$1" "$workflow" >/dev/null; then
        printf '%s\n' "error: CI policy is missing: $1" >&2
        exit 1
    fi
}

require_line 'branches: [master, candidate]'
require_line 'contents: read'
require_line 'persist-credentials: false'
require_line 'rustup toolchain install 1.98.0'
require_line 'cargo install wasm-tools --version 1.245.1 --locked'
require_line 'bubblewrap'
require_line 'shellcheck'
require_line 'tests/reject-published-change.sh'
require_line 'git show "$BASE_SHA:tests/reject-published-change.sh" >"$trusted_gate"'
require_line 'sh "$trusted_gate" pull_request "$REPOSITORY"'
require_line 'git show "$BASE_SHA:tests/check-community-change.mjs" >"$trusted_community_gate"'
require_line 'cargo run -p marketplace-tool --locked --quiet -- build-plan'
require_line 'node "$trusted_community_gate" "$PWD" "$build_plan" "$changed_paths"'
require_line 'sh tests/community-submission-smoke.sh'
require_line 'tests/sandbox-component-build-smoke.sh'
require_line 'scripts/stage-catalog-repository.sh --mode production'
require_line 'node --test tests/landing/*.test.mjs'
require_line 'node tests/site-runtime.test.js public/marketplace/v1/catalog.json'
require_line 'diff --recursive --no-dereference "${RUNNER_TEMP}/public-first" public'

if test "$(/usr/bin/grep -Fc 'scripts/build-local.sh' "$workflow")" -ne 2; then
    printf '%s\n' 'error: CI must build the local output exactly twice' >&2
    exit 1
fi

if /usr/bin/grep -Eq \
        'pull_request_target|pull-requests: write|contents: write|id-token: write|secrets\.|github\.token|GITHUB_TOKEN|upload-artifact|deploy|[Cc]oolify|build-production\.sh|--private-key' \
        "$workflow"; then
    printf '%s\n' 'error: CI has publication authority' >&2
    exit 1
fi

printf '%s\n' 'CI policy smoke tests passed'
