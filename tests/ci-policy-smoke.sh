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
component_sandbox="$repo_root/scripts/sandbox-component-build.sh"
published_verifier="$repo_root/scripts/verify-published.sh"
gate="$repo_root/tests/reject-published-change.sh"
community_gate="$repo_root/tests/check-community-change.mjs"
community_smoke="$repo_root/tests/community-submission-smoke.sh"
trusted_gate="$repo_root/tests/reject-trusted-change.sh"
trust_smoke="$repo_root/tests/ci-trust-boundary-smoke.sh"
sandbox_review_smoke="$repo_root/tests/sandbox-review-checks-smoke.sh"
review_gate="$repo_root/scripts/review-revision.sh"
codeowners="$repo_root/.github/CODEOWNERS"
testing_doc="$repo_root/docs/testing.md"

for owned_path in \
        '/tests/reject-published-change.sh @Valhallab' \
        '/tests/reject-trusted-change.sh @Valhallab' \
        '/tests/check-community-change.mjs @Valhallab' \
        '/tests/community-submission-smoke.sh @Valhallab' \
        '/tests/ci-trust-boundary-smoke.sh @Valhallab' \
        '/tests/sandbox-review-checks-smoke.sh @Valhallab' \
        '/tests/ci-policy-smoke.sh @Valhallab' \
        '/scripts/review-revision.sh @Valhallab'; do
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
if ! test -x "$review_gate" || test -L "$review_gate"; then
    printf '%s\n' 'error: maintainer review gate is missing or unsafe' >&2
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
require_line 'pull_request_target:'
require_line 'timeout-minutes: 15'
require_line 'contents: read'
require_line 'persist-credentials: false'
require_line 'rustup toolchain install 1.98.0'
require_line 'rustup target add --toolchain 1.98.0 wasm32-wasip2'
require_line 'PR_NUMBER: ${{ github.event.pull_request.number }}'
require_line 'CI_EVENT=pull_request'
require_line 'refs/pull/$PR_NUMBER/head:refs/overcrow-review/$REVIEW_SHA'
require_line '--no-tags --no-write-fetch-head'
require_line 'https://github.com/Valhallab/playervox-overcrow-marketplace.git'
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
require_line '--unshare-all --share-net'
require_line 'CARGO_NET_OFFLINE=true'

if ! /usr/bin/grep -F -x -- \
        '            "$private_root" admission' "$workflow" >/dev/null \
        || ! /usr/bin/grep -F -x -- \
            'verification_mode=${10:-full}' "$ci_driver" >/dev/null; then
    printf '%s\n' 'error: hosted CI does not select static admission explicitly' >&2
    exit 1
fi
if ! /usr/bin/grep -F -x -- '  pull_request_target:' "$workflow" >/dev/null \
        || /usr/bin/grep -F -x -- '  pull_request:' "$workflow" >/dev/null \
        || /usr/bin/grep -F -- 'allow-unsafe-pr-checkout:' "$workflow" >/dev/null \
        || /usr/bin/grep -E -- '^[[:space:]]+(ref|repository):' \
            "$workflow" >/dev/null; then
    printf '%s\n' 'error: pull-request admission workflow is candidate-controlled' >&2
    exit 1
fi
if ! /usr/bin/awk '
        /^  pull_request_target:$/ { section = "pull"; next }
        /^  push:$/ { section = "push"; next }
        /^  [^ ]/ { section = "" }
        /^    branches: \[master, candidate\]$/ {
            if (section == "pull") pull_branches += 1
            if (section == "push") push_branches += 1
        }
        END { exit !(pull_branches == 1 && push_branches == 1) }
    ' "$workflow"; then
    printf '%s\n' 'error: CI branch admission scope is not explicit' >&2
    exit 1
fi
if ! /usr/bin/awk '
        /case "\$PR_NUMBER" in/ { number = NR }
        /if test "\$\{#PR_NUMBER\}" -gt 20/ { length_check = NR }
        /ci_git fetch --no-tags --no-write-fetch-head/ { fetch = NR }
        /fetched_revision=\$\(ci_git rev-parse --verify/ { resolve = NR }
        /if test "\$fetched_revision" != "\$REVIEW_SHA"/ { compare = NR }
        END {
            exit !(number > 0 && length_check > number && fetch > length_check \
                && resolve > fetch && compare > resolve)
        }
    ' "$workflow"; then
    printf '%s\n' 'error: pull-request object fetch is not bounded and verified' >&2
    exit 1
fi
if ! /usr/bin/grep -F -- \
        'show "$trust_sha:scripts/materialize-git-snapshot.sh"' \
        "$review_gate" >/dev/null \
        || ! /usr/bin/grep -F -x -- \
            '    "$private_root" full' "$review_gate" >/dev/null; then
    printf '%s\n' 'error: maintainer gate does not bootstrap exact reviewed bytes' >&2
    exit 1
fi
if /usr/bin/grep -E \
        'apt-get|bubblewrap|shellcheck|systemctl' \
        "$workflow" >/dev/null; then
    printf '%s\n' 'error: hosted admission installs or starts sandbox tooling' >&2
    exit 1
fi

if ! /usr/bin/awk '
        /current_head=/ { head = NR }
        /if test "\$current_head" != "\$trust_sha"/ { head_reject = NR }
        /status_size=/ { clean = NR }
        /if test "\$status_size" -gt 1048576/ { clean_reject = NR }
        /sh "\$bootstrap" --bootstrap/ { materialized = NR }
        /"\$cargo_path" fetch --locked/ { fetches += 1; fetch = NR }
        /sh "\$trusted_root\/scripts\/ci-verify[.]sh"/ { verify = NR }
        END {
            exit !(head > 0 && head_reject > head && clean > head_reject \
                && clean_reject > clean && materialized > clean_reject \
                && fetches == 1 && fetch > materialized && verify > fetch)
        }
    ' "$review_gate" \
        || ! /usr/bin/grep -F -x -- \
            'if test "$current_head" != "$trust_sha" || test "$resolved_review" != "$review_sha"; then' \
            "$review_gate" >/dev/null \
        || ! /usr/bin/grep -F -x -- \
            'if test "$status_size" -gt 1048576 || test -s "$status_file"; then' \
            "$review_gate" >/dev/null \
        || /usr/bin/grep -F -- 'cargo fetch' "$testing_doc" >/dev/null; then
    printf '%s\n' 'error: maintainer dependency bootstrap precedes trust checks' >&2
    exit 1
fi
if ! /usr/bin/grep -F -- \
        'resolve-pinned-rust.sh"' \
        "$review_gate" >/dev/null \
        || ! /usr/bin/grep -F -x -- '    --fetch "$trusted_root") || exit 1' \
            "$review_gate" >/dev/null \
        || ! /usr/bin/grep -F -- '"$cargo_path" fetch --locked' \
            "$review_gate" >/dev/null \
        || /usr/bin/grep -E -- \
            '^[[:space:]]*cargo[[:space:]]+fetch' \
            "$review_gate" >/dev/null; then
    printf '%s\n' 'error: maintainer dependency bootstrap uses an unpinned Cargo' >&2
    exit 1
fi
if ! /usr/bin/awk '
        /if test "\$verification_mode" = admission/ { admission = NR }
        /stage-catalog-repository[.]sh" --mode production/ { stage = NR }
        END { exit !(admission > 0 && stage > admission) }
    ' "$ci_driver"; then
    printf '%s\n' 'error: hosted admission does not exit before candidate execution' >&2
    exit 1
fi

for sandbox in "$sandbox_driver" "$component_sandbox"; do
    if ! /usr/bin/grep -F -x \
            'system_gcc=$(sh "$script_dir/resolve-system-gcc.sh") || exit 1' \
            "$sandbox" >/dev/null \
            || ! /usr/bin/grep -F -- '"$system_gcc" -std=c11' \
                "$sandbox" >/dev/null \
            || /usr/bin/grep -F -- '/usr/bin/gcc -std=c11' \
                "$sandbox" >/dev/null; then
        printf '%s\n' 'error: sandbox does not use the trusted system compiler' >&2
        exit 1
    fi
done

for resource_limited_runner in \
        "$sandbox_driver" "$component_sandbox" "$published_verifier"; do
    if /usr/bin/grep -F -- 'systemd-run --user --scope' \
            "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- \
                'systemd-run --user --wait --pipe --collect' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- \
                '--quiet --expand-environment=no --service-type=exec' \
                "$resource_limited_runner" >/dev/null; then
        printf '%s\n' 'error: resource limits do not use a transient user service' >&2
        exit 1
    fi
    if /usr/bin/grep -F -- '--unshare-net' \
            "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- '--cap-add CAP_SYS_ADMIN' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- '--cap-add CAP_SETPCAP' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- \
                '/usr/bin/unshare --net /usr/bin/setpriv' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- '--bounding-set=-all' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- '--inh-caps=-all' \
                "$resource_limited_runner" >/dev/null \
            || ! /usr/bin/grep -F -- '--ambient-caps=-all' \
                "$resource_limited_runner" >/dev/null; then
        printf '%s\n' 'error: network sandbox does not drop setup capabilities' >&2
        exit 1
    fi
done

if ! /usr/bin/grep -F -- '--dir /proc --proc /proc' \
        "$published_verifier" >/dev/null; then
    printf '%s\n' 'error: published verifier does not expose isolated process status' >&2
    exit 1
fi

if ! /usr/bin/awk '
        /CDPATH='"'"''"'"' cd -- "\$trusted_root"/ { trusted_root_line = NR }
        /\/usr\/bin\/timeout --signal=TERM --kill-after=5 300/ {
            timeout_line = NR
        }
        /cargo fetch --locked/ {
            fetches += 1
            if (trusted_root_line == 0 || timeout_line == 0 \
                    || trusted_root_line >= timeout_line \
                    || timeout_line >= NR || NR - trusted_root_line > 3) {
                invalid = 1
            }
        }
        /cargo[[:space:]]+fetch/ && $0 !~ /cargo fetch --locked/ {
            invalid = 1
        }
        END { exit !(fetches == 1 && invalid == 0) }
    ' "$workflow"; then
    printf '%s\n' 'error: trusted dependency bootstrap is unsafe' >&2
    exit 1
fi

if /usr/bin/grep -Eq \
        '(^|[[:space:]])(cargo|node|sh)[[:space:]]+(fmt|clippy|test|tests/|scripts/)' \
        "$workflow"; then
    printf '%s\n' 'error: CI executes pull-request code outside the trusted driver' >&2
    exit 1
fi

if /usr/bin/grep -Eq \
        'pull-requests: write|contents: write|id-token: write|secrets\.|github\.token|GITHUB_TOKEN|upload-artifact|deploy|[Cc]oolify|build-production\.sh|--private-key' \
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
