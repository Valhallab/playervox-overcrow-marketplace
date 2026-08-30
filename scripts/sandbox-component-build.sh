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
for program in /usr/bin/timeout /usr/bin/prlimit /usr/bin/tar /usr/bin/env; do
    if test ! -f "$program" || test -L "$program" \
            || test "$(/usr/bin/stat -c '%u:%a' "$program")" != 0:755; then
        printf '%s\n' 'error: required resource control is unavailable' >&2
        exit 1
    fi
done

pin_file="$source_root/rust-toolchain.toml"
if test ! -f "$pin_file" || test -L "$pin_file" \
        || test "$(/usr/bin/stat -c '%u:%h' "$pin_file")" != "$invoking_uid:1" \
        || test "$(/usr/bin/stat -c '%s' "$pin_file")" -gt 4096 \
        || /usr/bin/find "$pin_file" -perm /0022 -print -quit | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
pinned_release=$(
    /usr/bin/awk '
        /^[[:space:]]*\[/ {
            in_toolchain = ($0 ~ /^[[:space:]]*\[toolchain\][[:space:]]*$/)
            next
        }
        /^[[:space:]]*channel[[:space:]]*=/ {
            count += 1
            if (!in_toolchain || $0 !~ /^[[:space:]]*channel[[:space:]]*=[[:space:]]*"1[.]98[.]0"[[:space:]]*$/) {
                exit 1
            }
        }
        END {
            if (count != 1) exit 1
            print "1.98.0"
        }
    ' "$pin_file"
) || {
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
}
if test "$pinned_release" != 1.98.0; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
case "$(/usr/bin/uname -m)" in
    x86_64) rust_host=x86_64-unknown-linux-gnu ;;
    aarch64) rust_host=aarch64-unknown-linux-gnu ;;
    *) printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2; exit 1 ;;
esac
user_root=$(/usr/bin/getent passwd "$invoking_uid" | /usr/bin/cut -d : -f 6)
case "$user_root" in
    /*) ;;
    *) printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2; exit 1 ;;
esac
if test "$user_root" = / || test ! -d "$user_root" || test -L "$user_root" \
        || test "$(CDPATH='' cd -- "$user_root" && pwd -P)" != "$user_root" \
        || test "$(/usr/bin/stat -c '%u' "$user_root")" != "$invoking_uid" \
        || /usr/bin/find "$user_root" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
toolchain_root="$user_root/.rustup/toolchains/$pinned_release-$rust_host"
toolchain_bin="$toolchain_root/bin"
cargo_path="$toolchain_bin/cargo"
rustc_path="$toolchain_bin/rustc"
caller_cargo=$(command -v cargo 2>/dev/null || true)
caller_cargo=$(/usr/bin/readlink -f -- "$caller_cargo" 2>/dev/null || true)
if test "$caller_cargo" != "$cargo_path" \
        || test ! -d "$toolchain_root" || test -L "$toolchain_root" \
        || test "$(/usr/bin/readlink -f -- "$toolchain_root")" != "$toolchain_root" \
        || test "$(/usr/bin/stat -c '%u' "$toolchain_root")" != "$invoking_uid"; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
cargo_home="$user_root/.cargo"
cargo_index="$cargo_home/registry/index"
cargo_cache="$cargo_home/registry/cache"
cargo_sources="$cargo_home/registry/src"
for required in \
        "$cargo_path" "$rustc_path" \
        "$toolchain_root/lib/rustlib/wasm32-wasip2" \
        "$cargo_index" "$cargo_cache" "$cargo_sources"; do
    if test ! -e "$required" || test -L "$required"; then
        printf '%s\n' 'error: required read-only build input is unavailable' >&2
        exit 1
    fi
done
for directory in \
        "$toolchain_root" "$toolchain_root/lib/rustlib/wasm32-wasip2" \
        "$cargo_index" "$cargo_cache" "$cargo_sources"; do
    if test ! -d "$directory" || test -L "$directory" \
            || test "$(/usr/bin/readlink -f -- "$directory")" != "$directory" \
            || test "$(/usr/bin/stat -c '%u' "$directory")" != "$invoking_uid" \
            || /usr/bin/find "$directory" -maxdepth 0 -perm /0022 -print -quit \
                | /usr/bin/grep . >/dev/null; then
        printf '%s\n' 'error: required read-only build input is unavailable' >&2
        exit 1
    fi
done
for binary in "$cargo_path" "$rustc_path"; do
    if test ! -f "$binary" || test -L "$binary" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$binary")" != "$invoking_uid:755:1"; then
        printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
        exit 1
    fi
done
version_file=$(/usr/bin/mktemp /tmp/marketplace-rustc-version.XXXXXXXXXX) || exit 1
artifact_archive="$target_root/.component-artifacts.tar"
artifact_directory="$target_root/artifacts"
cleanup_version() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$version_file"
    /usr/bin/rm -f -- "$artifact_archive"
    if test "$status" -ne 0 && test -n "$artifact_directory"; then
        /usr/bin/rm -rf -- "$artifact_directory"
    fi
    exit "$status"
}
trap cleanup_version EXIT HUP INT TERM
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 5 \
        /usr/bin/prlimit --cpu=5 --as=1073741824 --nofile=64 --fsize=4096 -- \
        "$rustc_path" --version --verbose >"$version_file" 2>/dev/null; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
release_count=$(/usr/bin/grep -c '^release: 1[.]98[.]0$' "$version_file" || :)
banner_count=$(/usr/bin/grep -c '^rustc 1[.]98[.]0 ' "$version_file" || :)
if test "$release_count" != 1 || test "$banner_count" != 1; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi

if ! /usr/bin/timeout --signal=TERM --kill-after=5 120 \
        /usr/bin/prlimit --cpu=20 --as=4294967296 --nproc=4096 \
            --nofile=256 --fsize=33554432 -- \
        "$bwrap_path" \
        --unshare-all \
        --unshare-net \
        --die-with-parent \
        --new-session \
        --cap-drop ALL \
        --clearenv \
        --ro-bind /usr /usr \
        --symlink usr/bin /bin \
        --symlink usr/lib /lib \
        --symlink usr/lib /lib64 \
        --proc /proc \
        --dev /dev \
        --tmpfs /tmp \
        --size 268435456 \
        --tmpfs /output \
        --dir /home \
        --dir /home/build \
        --dir /cargo-home \
        --dir /cargo-home/registry \
        --ro-bind "$cargo_index" /cargo-home/registry/index \
        --ro-bind "$cargo_cache" /cargo-home/registry/cache \
        --ro-bind "$cargo_sources" /cargo-home/registry/src \
        --ro-bind "$toolchain_root" /rust-toolchain \
        --dir /rustup-home \
        --ro-bind "$source_root" /source \
        --ro-bind "$build_plan" /build-plan.tsv \
        --chdir /source \
        --setenv PATH /rust-toolchain/bin:/usr/bin:/bin \
        --setenv HOME /home/build \
        --setenv CARGO_HOME /cargo-home \
        --setenv RUSTUP_HOME /rustup-home \
        --setenv RUSTC /rust-toolchain/bin/rustc \
        --setenv CARGO_NET_OFFLINE true \
        --setenv CARGO_TARGET_DIR /output/target \
        --setenv CARGO_INCREMENTAL 0 \
        --setenv CARGO_BUILD_JOBS 2 \
        --setenv LC_ALL C.UTF-8 \
        --setenv LANG C.UTF-8 \
        --setenv SOURCE_DATE_EPOCH 0 \
        --setenv RUSTFLAGS '--remap-path-prefix=/source=/usr/src/overcrow' \
        /bin/sh -c '
            if IFS= read -r inherited_input; then
                exit 1
            fi
            tab=$(printf "\t")
            set --
            while IFS="$tab" read -r package artifact source; do
                set -- "$@" -p "$package"
            done </build-plan.tsv
            /rust-toolchain/bin/cargo build \
                --manifest-path /source/Cargo.toml \
                --release \
                --target wasm32-wasip2 \
                --locked \
                --offline \
                --lib "$@" >/dev/null 2>&1 || exit 1
            set --
            while IFS="$tab" read -r package artifact source; do
                set -- "$@" "$artifact.wasm"
            done </build-plan.tsv
            cd /output/target/wasm32-wasip2/release || exit 1
            /usr/bin/tar --create --file=- -- "$@"
        ' </dev/null >"$artifact_archive" 2>/dev/null; then
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
/usr/bin/install -d -m 0700 "$artifact_directory"
if ! /usr/bin/tar --extract --file="$artifact_archive" \
        --directory="$artifact_directory" --no-same-owner --no-same-permissions \
        --no-xattrs --no-acls --no-selinux; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi
artifact_count=0
while IFS="$tab" read -r cargo_package component_artifact source_directory; do
    artifact="$artifact_directory/$component_artifact.wasm"
    if test ! -f "$artifact" || test -L "$artifact" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$artifact")" \
                != "$invoking_uid:600:1" \
            || test "$(/usr/bin/stat -c '%s' "$artifact")" -gt 4194304; then
        printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
        exit 1
    fi
    artifact_count=$((artifact_count + 1))
done <"$build_plan"
actual_artifacts=$(
    /usr/bin/find "$artifact_directory" -mindepth 1 -maxdepth 1 -type f -printf . \
        | /usr/bin/wc -c
)
if test "$artifact_count" -ne "$package_count" \
        || test "$actual_artifacts" -ne "$package_count" \
        || test -n "$(/usr/bin/find "$artifact_directory" -mindepth 1 ! -type f -print -quit)"; then
    printf '%s\n' 'error: sandboxed component artifacts are unsafe' >&2
    exit 1
fi
/usr/bin/rm -f -- "$artifact_archive"
artifact_directory=''
