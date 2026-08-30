#!/bin/sh
set -eu
umask 077

if test "$#" -ne 2; then
    printf '%s\n' 'usage: sandbox-component-build.sh ABSOLUTE-SOURCE-ROOT ABSOLUTE-TARGET-ROOT' >&2
    exit 2
fi

source_root=$1
target_root=$2
case "$source_root" in
    /*) ;;
    *) printf '%s\n' 'error: source root must be absolute' >&2; exit 1 ;;
esac
case "$target_root" in
    /*) ;;
    *) printf '%s\n' 'error: target root must be absolute' >&2; exit 1 ;;
esac
if test "$source_root" = / || test "$target_root" = / \
        || test ! -d "$source_root" || test -L "$source_root" \
        || test ! -d "$target_root" || test -L "$target_root" \
        || test "$(CDPATH='' cd -- "$source_root" && pwd -P)" != "$source_root" \
        || test "$(CDPATH='' cd -- "$target_root" && pwd -P)" != "$target_root"; then
    printf '%s\n' 'error: unsafe component build roots' >&2
    exit 1
fi

invoking_uid=$(/usr/bin/id -u)
build_plan="$target_root/build-plan.tsv"
if test ! -f "$build_plan" || test -L "$build_plan" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$build_plan")" != "$invoking_uid:600:1" \
        || test "$(/usr/bin/stat -c '%s' "$build_plan")" -gt 131072 \
        || test "$(/usr/bin/find "$target_root" -mindepth 1 -maxdepth 1 -printf '%f\n')" != build-plan.tsv; then
    printf '%s\n' 'error: unsafe validated build plan' >&2
    exit 1
fi
tab=$(printf '\t')
package_count=0
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    case "$cargo_package" in
        '' | *[!a-z0-9-]* | -* )
            printf '%s\n' 'error: unsafe validated build plan' >&2
            exit 1
            ;;
    esac
    case "$component_artifact" in
        '' | *[!a-z0-9_]* | _* )
            printf '%s\n' 'error: unsafe validated build plan' >&2
            exit 1
            ;;
    esac
    if test "${#cargo_package}" -gt 128 || test "${#component_artifact}" -gt 128 \
            || test "${#source_directory}" -gt 192; then
        printf '%s\n' 'error: unsafe validated build plan' >&2
        exit 1
    fi
    case "$source_directory" in
        '' | /* | */ | *[!A-Za-z0-9._/-]* | *//* | */../* | ../* | */.. | */./* | ./* | */.)
            printf '%s\n' 'error: unsafe validated build plan' >&2
            exit 1
            ;;
    esac
    package_count=$((package_count + 1))
    if test "$package_count" -gt 500; then
        printf '%s\n' 'error: unsafe validated build plan' >&2
        exit 1
    fi
done <"$build_plan"
if test "$package_count" -eq 0; then
    printf '%s\n' 'error: unsafe validated build plan' >&2
    exit 1
fi
if test "$(/usr/bin/stat -c '%u' "$source_root")" != "$invoking_uid" \
        || test "$(/usr/bin/stat -c '%u' "$target_root")" != "$invoking_uid" \
        || test "$(/usr/bin/stat -c '%a' "$target_root")" != 700 \
        || /usr/bin/find "$source_root" -maxdepth 0 -perm /0022 -print -quit | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: unsafe component build ownership' >&2
    exit 1
fi

bwrap_path=$(command -v bwrap 2>/dev/null || true)
case "$bwrap_path" in
    /usr/bin/bwrap | /bin/bwrap) ;;
    *) printf '%s\n' 'error: Bubblewrap is required for production builds' >&2; exit 1 ;;
esac
if test ! -f "$bwrap_path" || test -L "$bwrap_path" \
        || test "$(/usr/bin/stat -c '%u:%a' "$bwrap_path")" != 0:755; then
    printf '%s\n' 'error: Bubblewrap is unavailable or unsafe' >&2
    exit 1
fi
for program in \
        /usr/bin/timeout /usr/bin/prlimit /usr/bin/tar /usr/bin/env \
        /usr/bin/systemd-run /usr/bin/gcc /usr/bin/setpriv; do
    if test ! -f "$program" || test -L "$program" \
            || test "$(/usr/bin/stat -c '%u:%a' "$program")" != 0:755; then
        printf '%s\n' 'error: required resource control is unavailable' >&2
        exit 1
    fi
done
runtime_directory="/run/user/$invoking_uid"
session_bus="$runtime_directory/bus"
if test ! -d "$runtime_directory" || test -L "$runtime_directory" \
        || test "$(/usr/bin/stat -c '%u:%a' "$runtime_directory")" \
            != "$invoking_uid:700" \
        || test ! -S "$session_bus" || test -L "$session_bus" \
        || test "$(/usr/bin/stat -c '%u' "$session_bus")" != "$invoking_uid"; then
    printf '%s\n' 'error: delegated build resource scope is unavailable' >&2
    exit 1
fi

script_dir=$(CDPATH='' cd -- "$(/usr/bin/dirname -- "$0")" && pwd -P)
resolved_toolchain=$(sh "$script_dir/resolve-pinned-rust.sh" "$source_root") || exit 1
tab=$(printf '\t')
IFS="$tab" read -r toolchain_root cargo_path rustc_path \
    cargo_index cargo_cache cargo_sources <<EOF
$resolved_toolchain
EOF
if test -z "$cargo_sources"; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
artifact_archive="$target_root/.component-artifacts.tar"
artifact_directory="$target_root/artifacts"
supervisor_directory=$(/usr/bin/mktemp -d /tmp/marketplace-supervisor.XXXXXXXXXX) || exit 1
supervisor="$supervisor_directory/sandbox-supervisor"
cleanup_version() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$artifact_archive"
    /usr/bin/rm -rf -- "$supervisor_directory"
    if test "$status" -ne 0 && test -n "$artifact_directory"; then
        /usr/bin/rm -rf -- "$artifact_directory"
    fi
    exit "$status"
}
trap cleanup_version EXIT HUP INT TERM
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nproc=4096 \
            --nofile=64 --fsize=1048576 -- \
        /usr/bin/gcc -std=c11 -O2 -Wall -Wextra -Werror \
            "$script_dir/sandbox-supervisor.c" -o "$supervisor" \
            >/dev/null 2>&1 \
        || test ! -f "$supervisor" || test -L "$supervisor" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$supervisor")" \
            != "$invoking_uid:700:1" \
        || test "$(/usr/bin/stat -c '%s' "$supervisor")" -gt 1048576; then
    printf '%s\n' 'error: trusted sandbox supervisor is unavailable' >&2
    exit 1
fi
: >"$artifact_archive"
/usr/bin/chmod 0600 "$artifact_archive"

run_sandboxed_build() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        XDG_RUNTIME_DIR="$runtime_directory" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$session_bus" \
        /usr/bin/systemd-run --user --scope --quiet --collect \
        --property=MemoryMax=1073741824 \
        --property=MemorySwapMax=0 \
        --property=TasksMax=256 \
        --property=CPUQuota=200% \
        --property=RuntimeMaxSec=120 \
        /usr/bin/timeout --signal=TERM --kill-after=5 120 \
        /usr/bin/prlimit --cpu=20 --as=4294967296 --nproc=4096 \
            --nofile=256 --fsize=33554432 -- \
        "$bwrap_path" \
            --unshare-all --unshare-net --die-with-parent --new-session \
            --cap-drop ALL --clearenv \
            --ro-bind /usr /usr \
            --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib /lib64 \
            --dir /proc --proc /proc --dir /dev --dev /dev \
            --dir /build --size 268435456 --tmpfs /build \
            --dir /build/output --dir /build/tmp \
            --dir /build/home --dir /build/cargo-home \
            --dir /build/cargo-home/registry --dir /build/rustup-home \
            --dir /home --symlink ../build/home /home/build \
            --symlink build/tmp /tmp --symlink build/output /output \
            --symlink build/cargo-home /cargo-home \
            --symlink build/rustup-home /rustup-home \
            --ro-bind "$cargo_index" /build/cargo-home/registry/index \
            --ro-bind "$cargo_cache" /build/cargo-home/registry/cache \
            --ro-bind "$cargo_sources" /build/cargo-home/registry/src \
            --ro-bind "$toolchain_root" /rust-toolchain \
            --ro-bind "$supervisor" /sandbox-supervisor \
            --ro-bind "$source_root" /source \
            --ro-bind "$build_plan" /build-plan.tsv \
            --bind "$artifact_archive" /artifact-export \
            --chdir / --setenv PATH /usr/bin:/bin --setenv LC_ALL C.UTF-8 \
            /sandbox-supervisor /bin/sh -c '
                /usr/bin/cp -- /build-plan.tsv /build/compile-plan.tsv || exit 1
                /usr/bin/chmod 0444 /build/compile-plan.tsv || exit 1
                /usr/bin/setsid /usr/bin/setpriv \
                    --landlock-access fs:execute,write-file,read-file,read-dir,remove-dir,remove-file,make-dir,make-reg,make-sock,make-fifo,make-sym,refer,truncate \
                    --landlock-rule path-beneath:execute,read-file,read-dir:/usr \
                    --landlock-rule path-beneath:execute,read-file,read-dir:/rust-toolchain \
                    --landlock-rule path-beneath:read-file,read-dir:/source \
                    --landlock-rule path-beneath:read-file:/build/compile-plan.tsv \
                    --landlock-rule path-beneath:execute,write-file,read-file,read-dir,remove-dir,remove-file,make-dir,make-reg,make-sock,make-fifo,make-sym,refer,truncate:/build \
                    --landlock-rule path-beneath:read-file,read-dir:/proc \
                    --landlock-rule path-beneath:write-file,read-file,read-dir:/dev \
                    --no-new-privs /usr/bin/env -i \
                    PATH=/rust-toolchain/bin:/usr/bin:/bin \
                    HOME=/home/build TMPDIR=/tmp \
                    CARGO_HOME=/cargo-home RUSTUP_HOME=/rustup-home \
                    RUSTC=/rust-toolchain/bin/rustc CARGO_NET_OFFLINE=true \
                    CARGO_TARGET_DIR=/output/target CARGO_INCREMENTAL=0 \
                    CARGO_BUILD_JOBS=2 LC_ALL=C.UTF-8 LANG=C.UTF-8 \
                    SOURCE_DATE_EPOCH=0 \
                    RUSTFLAGS="--remap-path-prefix=/source=/usr/src/overcrow" \
                    /bin/sh -c '\''
                        tab=$(printf "\t")
                        set --
                        while IFS="$tab" read -r package artifact source; do
                            set -- "$@" -p "$package"
                        done </build/compile-plan.tsv
                        /rust-toolchain/bin/cargo build \
                            --manifest-path /source/Cargo.toml \
                            --release --target wasm32-wasip2 \
                            --locked --offline --lib "$@"
                    '\'' </dev/null >/dev/null 2>/dev/null || exit 1
                self_id=$(/bin/sh -c '\''printf "%s\n" "$PPID"'\'')
                for pass in 1 2 3; do
                    for process in /proc/[0-9]*; do
                        process_id=$(/usr/bin/basename -- "$process")
                        case "$process_id" in
                            1 | "$self_id") continue ;;
                        esac
                        /bin/kill -STOP "$process_id" 2>/dev/null || :
                        /bin/kill -KILL "$process_id" 2>/dev/null || :
                    done
                done
                for process in /proc/[0-9]*; do
                    process_id=$(/usr/bin/basename -- "$process")
                    case "$process_id" in
                        1 | "$self_id") continue ;;
                    esac
                    test ! -e "$process" || exit 1
                done
                tab=$(printf "\t")
                set --
                expected=0
                while IFS="$tab" read -r package artifact source; do
                    file="/build/output/target/wasm32-wasip2/release/$artifact.wasm"
                    test -f "$file" && test ! -L "$file" \
                        && test "$(/usr/bin/stat -c "%u" "$file")" \
                            = "$(/usr/bin/id -u)" \
                        && test "$(/usr/bin/stat -c "%s" "$file")" -le 4194304 \
                        || exit 1
                    set -- "$@" "$artifact.wasm"
                    expected=$((expected + 1))
                done </build-plan.tsv
                test "$expected" -gt 0 || exit 1
                cd /build/output/target/wasm32-wasip2/release || exit 1
                /usr/bin/tar --create --format=ustar --file=/artifact-export -- "$@"
            '
}
if ! run_sandboxed_build </dev/null >/dev/null 2>/dev/null; then
    printf '%s\n' 'error: sandboxed component build failed' >&2
    exit 1
fi
if test ! -f "$artifact_archive" || test -L "$artifact_archive" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$artifact_archive")" \
            != "$invoking_uid:600:1" \
        || test "$(/usr/bin/stat -c '%s' "$artifact_archive")" -gt 33554432; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi
if ! sh "$script_dir/extract-component-artifacts.sh" \
        "$artifact_archive" "$build_plan" "$artifact_directory"; then
    exit 1
fi
/usr/bin/rm -f -- "$artifact_archive"
artifact_directory=''
