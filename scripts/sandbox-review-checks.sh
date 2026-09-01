#!/bin/sh
set -eu
umask 077

mode=${1:-}
case "$mode:$#" in
    workspace:2 | workspace:3 | workspace:4) ;;
    site:3) ;;
    *)
        printf '%s\n' \
            'usage: sandbox-review-checks.sh workspace ABSOLUTE-SOURCE-ROOT [ABSOLUTE-PUBLIC-ROOT [ABSOLUTE-BUILD-PLAN]] | site ABSOLUTE-SOURCE-ROOT ABSOLUTE-PUBLIC-ROOT' >&2
        exit 2
        ;;
esac
source_root=$2
public_root=${3:-}
review_plan=${4:-}
targeted_review=false

safe_root() {
    root=$1
    case "$root" in
        /*) ;;
        *) return 1 ;;
    esac
    test "$root" != / && test -d "$root" && test ! -L "$root" \
        && test "$(CDPATH='' cd -- "$root" && pwd -P)" = "$root" \
        && test "$(/usr/bin/stat -c '%u' "$root")" = "$invoking_uid" \
        && test -z "$(/usr/bin/find "$root" -xdev ! -type d ! -type f -print -quit)" \
        && test -z "$(/usr/bin/find "$root" -xdev ! -user "$invoking_uid" -print -quit)" \
        && test -z "$(/usr/bin/find "$root" -xdev -perm /0022 -print -quit)" \
        && test "$(/usr/bin/find "$root" -xdev -type f -printf . | /usr/bin/wc -c)" -le 1500 \
        && test "$(/usr/bin/find "$root" -xdev -type f -printf '%s\n' \
            | /usr/bin/awk '{ total += $1 } END { print total + 0 }')" -le 536870912
}

invoking_uid=$(/usr/bin/id -u)
if ! safe_root "$source_root" \
        || { test -n "$public_root" && ! safe_root "$public_root"; }; then
    printf '%s\n' 'error: unsafe review-check roots' >&2
    exit 1
fi
if test -n "$review_plan"; then
    case "$review_plan" in /*) ;; *) review_plan='' ;; esac
    if test -z "$review_plan" || test ! -f "$review_plan" \
            || test -L "$review_plan" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$review_plan" \
                2>/dev/null || :)" != "$invoking_uid:600:1" \
            || test "$(/usr/bin/stat -c '%s' "$review_plan" 2>/dev/null || :)" \
                -gt 131072; then
        printf '%s\n' 'error: unsafe review build plan' >&2
        exit 1
    fi
    targeted_review=true
fi

script_dir=$(CDPATH='' cd -- "$(/usr/bin/dirname -- "$0")" && pwd -P)
bwrap_path=$(command -v bwrap 2>/dev/null || true)
case "$bwrap_path" in
    /usr/bin/bwrap | /bin/bwrap) ;;
    *) printf '%s\n' 'error: Bubblewrap is required for review checks' >&2; exit 1 ;;
esac
if test ! -f "$bwrap_path" || test -L "$bwrap_path" \
        || test "$(/usr/bin/stat -c '%u:%a' "$bwrap_path")" != 0:755; then
    printf '%s\n' 'error: Bubblewrap is unavailable or unsafe' >&2
    exit 1
fi
for program in \
        /usr/bin/timeout /usr/bin/prlimit /usr/bin/env /usr/bin/systemd-run \
        /usr/bin/readlink /usr/bin/setpriv /usr/bin/unshare; do
    if test ! -f "$program" || test -L "$program" \
            || test "$(/usr/bin/stat -c '%u:%a' "$program")" != 0:755; then
        printf '%s\n' 'error: required review resource control is unavailable' >&2
        exit 1
    fi
done
system_gcc=$(sh "$script_dir/resolve-system-gcc.sh") || exit 1
node_path=$(sh "$script_dir/resolve-system-node.sh") || exit 1

runtime_directory="/run/user/$invoking_uid"
session_bus="$runtime_directory/bus"
if test ! -d "$runtime_directory" || test -L "$runtime_directory" \
        || test "$(/usr/bin/stat -c '%u:%a' "$runtime_directory")" \
            != "$invoking_uid:700" \
        || test ! -S "$session_bus" || test -L "$session_bus" \
        || test "$(/usr/bin/stat -c '%u' "$session_bus")" != "$invoking_uid"; then
    printf '%s\n' 'error: delegated review resource scope is unavailable' >&2
    exit 1
fi

resolved_toolchain=$(sh "$script_dir/resolve-pinned-rust.sh" "$source_root") || exit 1
tab=$(printf '\t')
IFS="$tab" read -r toolchain_root _cargo_path _rustc_path \
    cargo_index cargo_cache cargo_sources <<EOF
$resolved_toolchain
EOF
if test -z "$cargo_sources"; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi

supervisor_directory=$(/usr/bin/mktemp -d /tmp/marketplace-review-supervisor.XXXXXXXXXX) \
    || exit 1
supervisor="$supervisor_directory/sandbox-supervisor"
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$supervisor_directory"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nproc=4096 \
            --nofile=64 --fsize=1048576 -- \
        "$system_gcc" -std=c11 -O2 -Wall -Wextra -Werror \
            "$script_dir/sandbox-supervisor.c" -o "$supervisor" \
            >/dev/null 2>&1 \
        || test ! -f "$supervisor" || test -L "$supervisor" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$supervisor")" \
            != "$invoking_uid:700:1" \
        || test "$(/usr/bin/stat -c '%s' "$supervisor")" -gt 1048576; then
    printf '%s\n' 'error: trusted review sandbox supervisor is unavailable' >&2
    exit 1
fi

set -- \
    --unshare-all --share-net --die-with-parent --new-session \
    --cap-add CAP_SYS_ADMIN --cap-add CAP_SETPCAP --clearenv \
    --ro-bind /usr /usr \
    --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib /lib64 \
    --dir /proc --proc /proc --dir /dev --dev /dev \
    --dir /build --size 3221225472 --tmpfs /build \
    --dir /build/target --dir /build/tmp --dir /build/home \
    --dir /build/cargo-home --dir /build/cargo-home/registry \
    --dir /build/rustup-home --dir /public \
    --dir /home --symlink ../build/home /home/build \
    --symlink build/tmp /tmp --symlink build/cargo-home /cargo-home \
    --symlink build/rustup-home /rustup-home \
    --ro-bind "$cargo_index" /build/cargo-home/registry/index \
    --ro-bind "$cargo_cache" /build/cargo-home/registry/cache \
    --ro-bind "$cargo_sources" /build/cargo-home/registry/src \
    --ro-bind "$toolchain_root" /rust-toolchain \
    --ro-bind "$node_path" /system-node \
    --ro-bind "$supervisor" /sandbox-supervisor \
    --ro-bind "$source_root" /source
if test -n "$public_root"; then
    set -- "$@" --ro-bind "$public_root" /public
fi
review_plan_mount=${review_plan:-/dev/null}
set -- "$@" --ro-bind "$review_plan_mount" /review-plan.tsv

if ! {
    # shellcheck disable=SC2016 # The sandbox child shell expands this review program.
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        XDG_RUNTIME_DIR="$runtime_directory" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$session_bus" \
        /usr/bin/systemd-run --user --wait --pipe --collect \
        --quiet --expand-environment=no --service-type=exec \
        --property=MemoryMax=5368709120 \
        --property=MemorySwapMax=0 \
        --property=TasksMax=256 \
        --property=CPUQuota=200% \
        --property=RuntimeMaxSec=300 \
        /usr/bin/timeout --signal=TERM --kill-after=5 300 \
        /usr/bin/prlimit --cpu=240 --as=8589934592 --nproc=4096 \
            --nofile=256 --fsize=268435456 -- \
        "$bwrap_path" "$@" \
        --chdir /source --setenv PATH /usr/bin:/bin --setenv LC_ALL C.UTF-8 \
        /usr/bin/unshare --net /usr/bin/setpriv \
            --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
            --no-new-privs /sandbox-supervisor /usr/bin/setpriv \
            --landlock-access fs:execute,write-file,read-file,read-dir,remove-dir,remove-file,make-dir,make-reg,make-sock,make-fifo,make-sym,refer,truncate \
            --landlock-rule path-beneath:execute,read-file,read-dir:/usr \
            --landlock-rule path-beneath:execute,read-file,read-dir:/rust-toolchain \
            --landlock-rule path-beneath:execute,read-file:/system-node \
            --landlock-rule path-beneath:read-file:/review-plan.tsv \
            --landlock-rule path-beneath:read-file,read-dir:/source \
            --landlock-rule path-beneath:read-file,read-dir:/public \
            --landlock-rule path-beneath:execute,write-file,read-file,read-dir,remove-dir,remove-file,make-dir,make-reg,make-sock,make-fifo,make-sym,refer,truncate:/build \
            --landlock-rule path-beneath:read-file,read-dir:/proc \
            --landlock-rule path-beneath:write-file,read-file,read-dir:/dev \
            --no-new-privs /usr/bin/env -i \
            PATH=/rust-toolchain/bin:/usr/bin:/bin \
            HOME=/home/build TMPDIR=/build/tmp \
            CARGO_HOME=/cargo-home RUSTUP_HOME=/rustup-home \
            RUSTC=/rust-toolchain/bin/rustc CARGO_NET_OFFLINE=true \
            CARGO_TARGET_DIR=/build/target CARGO_INCREMENTAL=0 \
            CARGO_BUILD_JOBS=2 LC_ALL=C.UTF-8 LANG=C.UTF-8 \
            OVERCROW_MARKETPLACE_TEST_PUBLIC=/public \
            SOURCE_DATE_EPOCH=0 \
            RUSTFLAGS='--remap-path-prefix=/source=/usr/src/overcrow' \
            /bin/sh -c '
                set -eu
                umask 022
                case "$1" in
                    workspace)
                        /rust-toolchain/bin/cargo fmt --all -- --check \
                            || exit 1
                        if test "$3" = true; then
                            tab=$(printf "\t")
                            set --
                            while IFS="$tab" read -r package artifact source extra; do
                                test -z "$extra" && test -n "$package" \
                                    && test -n "$artifact" && test -n "$source" \
                                    || exit 1
                                set -- "$@" -p "$package"
                            done </review-plan.tsv
                            test "$#" -eq 0 \
                                || /rust-toolchain/bin/cargo test \
                                    --all-targets --locked --offline "$@"
                        else
                            /rust-toolchain/bin/cargo test \
                                --workspace --all-targets --locked --offline
                        fi
                        ;;
                    site)
                        "$2" --test /source/tests/landing/*.test.mjs \
                            && "$2" /source/tests/site-runtime.test.js \
                                /public/marketplace/v1/catalog.json
                        ;;
                    *) exit 1 ;;
                esac
            ' sh "$mode" /system-node "$targeted_review" </dev/null
}; then
    printf '%s\n' 'error: sandboxed review checks failed' >&2
    exit 1
fi
