#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: verify-deployment-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
server_script="$repo_root/tests/deployment-fixture-server.py"
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-deployment-smoke.XXXXXXXXXX)
server_pid=
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if test -n "$server_pid"; then
        /usr/bin/kill "$server_pid" 2>/dev/null || :
        wait "$server_pid" 2>/dev/null || :
    fi
    /usr/bin/rm -rf -- "$scratch"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

for required in scripts/verify-deployment.sh scripts/verify-deployment-lib.sh \
        scripts/prepare-marketplace-tool.sh \
        "$server_script"; do
    if test ! -f "$repo_root/$required" && test ! -f "$required"; then
        printf '%s\n' "error: deployment fixture prerequisite is unavailable: $required" >&2
        exit 1
    fi
done

expect_direct_library_rejection() {
    label=$1
    shift
    if "$@" >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        printf '%s\n' "error: $label direct library execution was accepted" >&2
        exit 1
    fi
    printf '%s\n' "case=$label result=rejected"
}

expect_direct_library_rejection direct-library \
    /bin/sh "$repo_root/scripts/verify-deployment-lib.sh"
expect_direct_library_rejection direct-library-arguments \
    /bin/sh "$repo_root/scripts/verify-deployment-lib.sh" unexpected arguments
expect_direct_library_rejection direct-library-spoofed-context \
    /usr/bin/env MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT=production-wrapper \
    /bin/sh "$repo_root/scripts/verify-deployment-lib.sh"
if /bin/sh -eu -c '. "$1"' missing-context \
        "$repo_root/scripts/verify-deployment-lib.sh" \
        >"$scratch/missing-context.stdout" 2>"$scratch/missing-context.stderr"; then
    printf '%s\n' 'error: deployment library accepted a missing source context' >&2
    exit 1
fi
printf '%s\n' 'case=missing-source-context result=rejected'

# The production wrapper must pass the fixed origin even when ordinary
# environment variables request the old generic test override. Replace only
# its slow verification core in this copied wrapper boundary test.
wrapper_fixture="$scratch/wrapper"
/usr/bin/install -d -m 0755 "$wrapper_fixture/scripts" "$wrapper_fixture/published" \
    "$wrapper_fixture/keys"
/usr/bin/cp -- "$repo_root/scripts/verify-deployment.sh" \
    "$wrapper_fixture/scripts/verify-deployment.sh"
wrapper_capture="$scratch/wrapper-origin"
# shellcheck disable=SC2016 # The generated source expands this context variable.
printf '%s\n' \
    'if ! (return 0 2>/dev/null); then return 1; fi' \
    'test "${MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT-}" = production-wrapper || return 1' \
    'unset MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT' \
    'verify_marketplace_production_deployment() {' \
    "  printf '%s\\n' \"\$2\" >'$wrapper_capture'" \
    '}' >"$wrapper_fixture/scripts/verify-deployment-lib.sh"
: >"$wrapper_fixture/keys/overcrow-production-2026-01.pub"
MARKETPLACE_DEPLOYMENT_TEST_MODE=1 \
MARKETPLACE_DEPLOYMENT_TEST_ORIGIN=http://127.0.0.1:9 \
MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT=fixture-runner \
    sh "$wrapper_fixture/scripts/verify-deployment.sh" \
        "$wrapper_fixture/published" \
        "$wrapper_fixture/keys/overcrow-production-2026-01.pub" \
        overcrow-production-2026-01
wrapper_actual=$(/usr/bin/cat "$wrapper_capture")
if test "$wrapper_actual" != https://overcrow.playervox.com; then
    printf '%s\n' 'error: production wrapper origin was not fixed' >&2
    exit 1
fi
printf '%s\n' 'case=fixed-production-origin result=pass'

fixture="$scratch/repository"
/usr/bin/install -d -m 0700 -- "$fixture"
for path in .gitignore Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures \
        providers widgets sdk wit examples tools web scripts tests; do
    /usr/bin/cp -R -- "$repo_root/$path" "$fixture/"
done
/usr/bin/git init --quiet "$fixture"
/usr/bin/git -C "$fixture" config user.name 'Marketplace Deployment Tests'
/usr/bin/git -C "$fixture" config user.email 'deployment-tests@invalid.example'
/usr/bin/git -C "$fixture" checkout --quiet -b release/fixture
/usr/bin/install -d -m 0755 -- "$fixture/published"
printf '%s\n' prior >"$fixture/published/prior.txt"

tool_work="$scratch/trusted-tool"
/usr/bin/install -d -m 0700 -- "$tool_work"
trusted_tool=$(sh "$fixture/scripts/prepare-marketplace-tool.sh" "$fixture" "$tool_work")
authority="$scratch/authority"
/usr/bin/install -d -m 0700 -- "$authority" "$fixture/keys"
printf '%s\n' \
    2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a \
    >"$authority/signing.key"
printf '%s\n' 1 >"$authority/sequence.txt"
/usr/bin/chmod 0600 "$authority/signing.key" "$authority/sequence.txt"
"$trusted_tool" derive-public-key --repository "$fixture" \
    --signing-key "$authority/signing.key" \
    --key-id overcrow-production-2026-01 \
    --output "$fixture/keys/overcrow-production-2026-01.pub" >/dev/null
/usr/bin/cp -- "$fixture/web/landing/avatar-noct.png" \
    "$fixture/widgets/warframe-fissures/preview.png"
/usr/bin/python3 -c '
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
listing = json.loads(path.read_text(encoding="utf-8"))
listing["previewFile"] = "preview.png"
path.write_text(json.dumps(listing, indent=2) + "\n", encoding="utf-8")
' "$fixture/widgets/warframe-fissures/listing.json"
/usr/bin/git -C "$fixture" add --all
/usr/bin/git -C "$fixture" commit --quiet -m 'deployment fixture with ephemeral production key'

source_fixture=$fixture
fixture="$scratch/staged-repository"
sh "$source_fixture/scripts/stage-catalog-repository.sh" \
    --mode production "$fixture" >/dev/null
printf '%s\n' 'case=fixture-stage result=pass'

assemble_public() {
    /usr/bin/rm -rf -- "$fixture/public"
    /usr/bin/install -d -m 0755 -- "$fixture/public"
    /usr/bin/cp -R -- "$fixture/web/landing/." "$fixture/public/"
    /usr/bin/install -d -m 0755 -- "$fixture/public/marketplace"
    for file in index.html app.js styles.css; do
        /usr/bin/install -m 0644 -- "$fixture/web/marketplace/$file" \
            "$fixture/public/marketplace/$file"
    done
    /usr/bin/install -m 0644 -- \
        "$fixture/web/marketplace/policies/production.js" \
        "$fixture/public/marketplace/catalog-policy.js"
    /usr/bin/find "$fixture/public" -type d -exec /usr/bin/chmod 0755 {} +
    /usr/bin/find "$fixture/public" -type f -exec /usr/bin/chmod 0644 {} +
}

publish_fixture() {
    assemble_public
    generated_at=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%SZ')
    expires_at=$(/usr/bin/date -u -d "$generated_at +90 days" '+%Y-%m-%dT%H:%M:%SZ')
    "$trusted_tool" build --repository "$fixture" \
        --generated-at "$generated_at" --expires-at "$expires_at" --production \
        --sequence-file "$authority/sequence.txt" \
        --sequence-state "$authority/state.json" \
        --signing-key "$authority/signing.key" \
        --key-id overcrow-production-2026-01
    /usr/bin/find "$fixture/public" -type d -exec /usr/bin/chmod 0755 {} +
    /usr/bin/find "$fixture/public" -type f -exec /usr/bin/chmod 0644 {} +
    "$trusted_tool" verify "$fixture/public/marketplace/v1/catalog.json" \
        --public-key "$fixture/keys/overcrow-production-2026-01.pub" \
        --key-id overcrow-production-2026-01 >/dev/null
    "$trusted_tool" verify-tree --repository "$fixture" --tree "$fixture/public" \
        --public-key "$fixture/keys/overcrow-production-2026-01.pub" \
        --key-id overcrow-production-2026-01 >/dev/null
}

publish_fixture
expected_published="$scratch/expected-published"
/usr/bin/cp -a -- "$fixture/public" "$expected_published"
"$trusted_tool" advance-sequence --repository "$fixture" \
    --sequence-file "$authority/sequence.txt" \
    --sequence-state "$authority/state.json" \
    --catalog "$fixture/public/marketplace/v1/catalog.json" >/dev/null
publish_fixture
different_catalog="$scratch/different-catalog.json"
/usr/bin/cp -- "$fixture/public/marketplace/v1/catalog.json" "$different_catalog"
/usr/bin/rm -rf -- "$fixture/published"
/usr/bin/cp -a -- "$expected_published" "$fixture/published"
printf '%s\n' 'case=signed-release-fixtures result=pass'

# Compiling the authority tool is not the behavior under test. Keep the real
# binary, but replace the copied repository's compiler helper with a bounded
# fixture adapter so every independent case verifies with identical trusted
# bytes without recompiling them.
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$#" -eq 2' \
    "printf '%s\\n' '$trusted_tool'" \
    >"$fixture/scripts/prepare-marketplace-tool.sh"
/usr/bin/chmod 0755 "$fixture/scripts/prepare-marketplace-tool.sh"

public_key="$fixture/keys/overcrow-production-2026-01.pub"
catalog_relative=marketplace/v1/catalog.json
package_relative=$(/usr/bin/find "$fixture/published/marketplace/v1/packages" \
    -type f -name '*.ocpkg' -printf '%P\n' | LC_ALL=C /usr/bin/sort | /usr/bin/head -n 1)
package_relative="marketplace/v1/packages/$package_relative"
preview_relative=$(/usr/bin/find "$fixture/published/marketplace/v1/previews" \
    -type f -name '*.png' -printf '%P\n' | LC_ALL=C /usr/bin/sort | /usr/bin/head -n 1)
preview_relative="marketplace/v1/previews/$preview_relative"
if test ! -f "$fixture/published/$package_relative"; then
    printf '%s\n' 'error: package fixture object is unavailable' >&2
    exit 1
fi
if test ! -f "$fixture/published/$preview_relative"; then
    printf '%s\n' 'error: preview fixture object is unavailable' >&2
    exit 1
fi
printf '%s\n' 'case=fixture-object-discovery result=pass'

run_checker() {
    origin=$1
    # The production wrapper never selects this mode. This source-level test
    # entry point accepts only a numeric loopback origin.
    /bin/sh -eu -c '
unset MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT
MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT=fixture-runner
. "$1"
shift
verify_marketplace_fixture_deployment "$@"
' fixture-runner "$fixture/scripts/verify-deployment-lib.sh" \
        "$fixture" "$origin" "$fixture/published" \
        "$public_key" overcrow-production-2026-01
}

start_server() {
    label=$1
    mode=$2
    target=${3:-}
    replacement=${4:-}
    port_file="$scratch/$label.port"
    request_log="$scratch/$label.requests"
    server_stdout="$scratch/$label.server.stdout"
    server_stderr="$scratch/$label.server.stderr"
    : >"$request_log"
    set -- /usr/bin/python3 "$server_script" \
        --root "$fixture/published" --port-file "$port_file" \
        --log-file "$request_log" --mode "$mode"
    if test -n "$target"; then
        set -- "$@" --target "$target"
    fi
    if test -n "$replacement"; then
        set -- "$@" --replacement "$replacement"
    fi
    "$@" >"$server_stdout" 2>"$server_stderr" &
    server_pid=$!
    attempts=0
    while test ! -s "$port_file"; do
        attempts=$((attempts + 1))
        if test "$attempts" -ge 100 || ! /usr/bin/kill -0 "$server_pid" 2>/dev/null; then
            printf '%s\n' "error: $label fixture server did not start" >&2
            /usr/bin/cat "$server_stderr" >&2
            exit 1
        fi
        /usr/bin/sleep 0.05
    done
    port=$(/usr/bin/cat "$port_file")
    case "$port" in '' | *[!0-9]*) exit 1 ;; esac
    origin="http://127.0.0.1:$port"
}

stop_server() {
    /usr/bin/kill "$server_pid" 2>/dev/null || :
    wait "$server_pid" 2>/dev/null || :
    server_pid=
}

expect_pass() {
    label=$1
    mode=$2
    target=${3:-}
    replacement=${4:-}
    start_server "$label" "$mode" "$target" "$replacement"
    if ! run_checker "$origin" >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        stop_server
        printf '%s\n' "error: $label deployment fixture was rejected" >&2
        /usr/bin/cat "$scratch/$label.stderr" >&2
        exit 1
    fi
    stop_server
    if test -s "$scratch/$label.stdout" || test -s "$scratch/$label.stderr"; then
        printf '%s\n' "error: $label deployment fixture produced diagnostics" >&2
        /usr/bin/cat "$scratch/$label.stdout" >&2
        /usr/bin/cat "$scratch/$label.stderr" >&2
        exit 1
    fi
    printf '%s\n' "case=$label result=pass"
}

expect_failure() {
    label=$1
    mode=$2
    target=${3:-}
    replacement=${4:-}
    start_server "$label" "$mode" "$target" "$replacement"
    if run_checker "$origin" >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        stop_server
        printf '%s\n' "error: $label deployment defect was accepted" >&2
        exit 1
    fi
    stop_server
    printf '%s\n' "case=$label result=rejected"
}

expect_pass valid valid
expected_requests="$scratch/valid.expected"
printf '%s\n' / /marketplace/ /marketplace/v1/catalog.json >"$expected_requests"
/usr/bin/find "$fixture/published" -xdev -type f -printf '/%P\n' \
    | LC_ALL=C /usr/bin/sort \
    | /usr/bin/grep -F -x -v /index.html \
    | /usr/bin/grep -F -x -v /marketplace/index.html \
    | /usr/bin/grep -F -x -v /marketplace/v1/catalog.json \
    >>"$expected_requests"
/usr/bin/cmp -- "$expected_requests" "$scratch/valid.requests"

expect_failure redirect redirect
test "$(/usr/bin/cat "$scratch/redirect.requests")" = /

expect_failure catalog-mismatch catalog-mismatch "$catalog_relative" "$different_catalog"
/usr/bin/grep -F -x /marketplace/v1/catalog.json \
    "$scratch/catalog-mismatch.requests" >/dev/null

expect_failure package-bytes bytes-mismatch "$package_relative"
/usr/bin/grep -F -x "/$package_relative" "$scratch/package-bytes.requests" >/dev/null

expect_failure preview-bytes bytes-mismatch "$preview_relative"
/usr/bin/grep -F -x "/$preview_relative" "$scratch/preview-bytes.requests" >/dev/null

expect_failure wrong-cache wrong-cache "$package_relative"
/usr/bin/grep -F -x "/$package_relative" "$scratch/wrong-cache.requests" >/dev/null

expect_failure duplicate-cache duplicate-cache "$package_relative"
/usr/bin/grep -F -x "/$package_relative" "$scratch/duplicate-cache.requests" >/dev/null

expect_failure wrong-mime wrong-mime "$preview_relative"
/usr/bin/grep -F -x "/$preview_relative" "$scratch/wrong-mime.requests" >/dev/null

expect_failure duplicate-mime duplicate-mime "$preview_relative"
/usr/bin/grep -F -x "/$preview_relative" "$scratch/duplicate-mime.requests" >/dev/null

expect_failure oversized oversized index.html
/usr/bin/grep -Eq '^OVERSIZED sent=[0-9]+ broken=1$' \
    "$scratch/oversized.requests"
oversized_sent=$(/usr/bin/sed -n 's/^OVERSIZED sent=\([0-9][0-9]*\) broken=1$/\1/p' \
    "$scratch/oversized.requests")
test "$oversized_sent" -lt 33554432

start_server excessive-count valid
/usr/bin/install -d -m 0755 "$fixture/published/count-fixture"
count=0
while test "$count" -le 1000; do
    : >"$fixture/published/count-fixture/$count"
    count=$((count + 1))
done
if run_checker "$origin" >"$scratch/excessive-count.stdout" \
        2>"$scratch/excessive-count.stderr"; then
    printf '%s\n' 'error: excessive local file count was accepted' >&2
    exit 1
fi
test ! -s "$scratch/excessive-count.requests"
/usr/bin/rm -rf -- "$fixture/published/count-fixture"
stop_server
printf '%s\n' 'case=excessive-count result=rejected-before-request'

start_server excessive-aggregate valid
/usr/bin/install -d -m 0755 "$fixture/published/aggregate-fixture"
count=0
while test "$count" -lt 17; do
    /usr/bin/truncate -s 16777216 "$fixture/published/aggregate-fixture/$count"
    count=$((count + 1))
done
if run_checker "$origin" >"$scratch/excessive-aggregate.stdout" \
        2>"$scratch/excessive-aggregate.stderr"; then
    printf '%s\n' 'error: excessive local aggregate size was accepted' >&2
    exit 1
fi
test ! -s "$scratch/excessive-aggregate.requests"
/usr/bin/rm -rf -- "$fixture/published/aggregate-fixture"
stop_server
printf '%s\n' 'case=excessive-aggregate result=rejected-before-request'

start_server symlink valid
/usr/bin/ln -s index.html "$fixture/published/symlink-fixture"
if run_checker "$origin" >"$scratch/symlink.stdout" 2>"$scratch/symlink.stderr"; then
    printf '%s\n' 'error: local symlink was accepted' >&2
    exit 1
fi
test ! -s "$scratch/symlink.requests"
/usr/bin/rm -f -- "$fixture/published/symlink-fixture"
stop_server
printf '%s\n' 'case=symlink result=rejected-before-request'

start_server nonregular valid
/usr/bin/mkfifo "$fixture/published/nonregular-fixture"
if run_checker "$origin" >"$scratch/nonregular.stdout" \
        2>"$scratch/nonregular.stderr"; then
    printf '%s\n' 'error: local nonregular entry was accepted' >&2
    exit 1
fi
test ! -s "$scratch/nonregular.requests"
/usr/bin/rm -f -- "$fixture/published/nonregular-fixture"
stop_server
printf '%s\n' 'case=nonregular result=rejected-before-request'

printf '%s\n' 'Deployment verifier smoke tests passed'
