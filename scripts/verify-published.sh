#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3; then
    printf '%s\n' 'usage: verify-published.sh ABSOLUTE-TREE ABSOLUTE-PUBLIC-KEY overcrow-production-2026-01' >&2
    exit 2
fi
tree=$1
public_key=$2
key_id=$3
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(/usr/bin/dirname -- "$script_dir")
case "$tree:$public_key" in /*:/*) ;; *)
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
    ;;
esac
if test "$key_id" != overcrow-production-2026-01 \
        || test ! -d "$tree" || test -L "$tree" \
        || test "$(CDPATH='' cd -- "$tree" && pwd -P)" != "$tree"; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

if test "$tree" = "$repo_root/published" || test "$tree" = "$repo_root/public"; then
    verification_repository=$repo_root
else
    verification_repository=${tree%/public}
    verification_parent=${verification_repository%/repository}
    case "$verification_parent" in
        "$repo_root"/.build-production.*) ;;
        *)
            printf '%s\n' 'error: published tree rejected' >&2
            exit 1
            ;;
    esac
fi
if test "$public_key" \
        != "$verification_repository/keys/overcrow-production-2026-01.pub"; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

work=$(/usr/bin/mktemp -d /tmp/marketplace-verify.XXXXXXXXXX)
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
tool_work="$work/tool"
/usr/bin/install -d -m 0700 "$tool_work"
trusted_tool=$(sh "$verification_repository/scripts/prepare-marketplace-tool.sh" \
    "$verification_repository" "$tool_work" 2>/dev/null) || {
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
}
catalog="$tree/marketplace/v1/catalog.json"
if ! "$trusted_tool" verify "$catalog" --public-key "$public_key" \
        --key-id "$key_id" >/dev/null 2>&1 \
        || ! "$trusted_tool" verify-tree --repository "$verification_repository" \
            --tree "$tree" --public-key "$public_key" --key-id "$key_id" \
            >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

bwrap_path=$(command -v bwrap 2>/dev/null || :)
case "$bwrap_path" in /usr/bin/bwrap | /bin/bwrap) ;; *) bwrap_path='' ;; esac
node_candidate=$(command -v node 2>/dev/null || :)
case "$node_candidate" in /*) ;; *) node_candidate='' ;; esac
node_path=$(/usr/bin/readlink -f -- "$node_candidate" 2>/dev/null || :)
if test -z "$bwrap_path" || test ! -f "$bwrap_path" || test -L "$bwrap_path" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$bwrap_path" 2>/dev/null || :)" \
            != 0:755:1 \
        || test -z "$node_path" || test "$node_candidate" != "$node_path" \
        || test ! -f "$node_path" || test -L "$node_path" \
        || test ! -x "$node_path" \
        || test "$(/usr/bin/stat -c '%u:%h' "$node_path" 2>/dev/null || :)" != 0:1 \
        || test "$(/usr/bin/stat -c '%s' "$node_path" 2>/dev/null || :)" \
            -gt 268435456 \
        || /usr/bin/find "$node_path" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi
node_directory=${node_path%/*}
while :; do
    if test ! -d "$node_directory" || test -L "$node_directory" \
            || test "$(/usr/bin/stat -c '%u' "$node_directory" 2>/dev/null || :)" != 0 \
            || /usr/bin/find "$node_directory" -maxdepth 0 -perm /0022 \
                -print -quit | /usr/bin/grep . >/dev/null; then
        printf '%s\n' 'error: published tree rejected' >&2
        exit 1
    fi
    test "$node_directory" = / && break
    node_parent=${node_directory%/*}
    test -n "$node_parent" || node_parent=/
    if test "$node_parent" = "$node_directory"; then
        printf '%s\n' 'error: published tree rejected' >&2
        exit 1
    fi
    node_directory=$node_parent
done
for program in /usr/bin/env /usr/bin/prlimit /usr/bin/systemd-run \
        /usr/bin/setpriv /usr/bin/timeout /usr/bin/unshare; do
    if test ! -f "$program" || test -L "$program" \
            || test "$(/usr/bin/stat -c '%u:%a' "$program" 2>/dev/null || :)" != 0:755; then
        printf '%s\n' 'error: published tree rejected' >&2
        exit 1
    fi
done
invoking_uid=$(/usr/bin/id -u)
runtime_directory="/run/user/$invoking_uid"
session_bus="$runtime_directory/bus"
if test ! -d "$runtime_directory" || test -L "$runtime_directory" \
        || test "$(/usr/bin/stat -c '%u:%a' "$runtime_directory" 2>/dev/null || :)" \
            != "$invoking_uid:700" \
        || test ! -S "$session_bus" || test -L "$session_bus" \
        || test "$(/usr/bin/stat -c '%u' "$session_bus" 2>/dev/null || :)" \
            != "$invoking_uid"; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi

run_node_sandbox() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 \
        XDG_RUNTIME_DIR="$runtime_directory" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$session_bus" \
        /usr/bin/systemd-run --user --wait --pipe --collect \
        --quiet --expand-environment=no --service-type=exec \
        --property=MemoryMax=536870912 \
        --property=MemorySwapMax=0 \
        --property=TasksMax=64 \
        --property=CPUQuota=100% \
        --property=RuntimeMaxSec=30 \
        /usr/bin/timeout --signal=KILL 30 \
        /usr/bin/prlimit --cpu=15 --as=4294967296 --nproc=4096 \
            --nofile=64 --fsize=1048576 -- \
        "$bwrap_path" \
            --unshare-all --share-net --die-with-parent --new-session \
            --cap-add CAP_SYS_ADMIN --cap-add CAP_SETPCAP --clearenv \
            --ro-bind /usr /usr \
            --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib /lib64 \
            --dev /dev --tmpfs /tmp --dir /home \
            --dir /workspace --dir /workspace/web \
            --ro-bind "$node_path" /node \
            --ro-bind "$tree" /tree \
            --ro-bind "$verification_repository/tests" /workspace/tests \
            --ro-bind "$tree/marketplace" /workspace/web/marketplace \
            --chdir /workspace \
            --setenv PATH /usr/bin:/bin --setenv LC_ALL C.UTF-8 \
            /usr/bin/unshare --net /usr/bin/setpriv \
                --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
                --no-new-privs "$@"
}
for landing_test in effects.test.mjs landing-content.test.mjs static-hygiene.test.mjs; do
    if ! run_node_sandbox /node "/workspace/tests/landing/$landing_test" /tree \
            >/dev/null 2>&1; then
        printf '%s\n' 'error: published tree rejected' >&2
        exit 1
    fi
done
if ! run_node_sandbox /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 \
        MARKETPLACE_CATALOG_PATH=/tree/marketplace/v1/catalog.json \
        MARKETPLACE_POLICY_PATH=/tree/marketplace/catalog-policy.js \
        /node --test \
            --test-name-pattern='production mode renders complete catalog metadata' \
            /workspace/tests/site-runtime.test.js >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree rejected' >&2
    exit 1
fi
