#!/bin/sh
set -eu

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
set --
if test -e "$build_plan" || test -L "$build_plan"; then
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
        set -- "$@" -p "$cargo_package"
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
elif test -n "$(/usr/bin/find "$target_root" -mindepth 1 -print -quit)"; then
    printf '%s\n' 'error: unsafe component target root' >&2
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

cargo_path=$(command -v cargo 2>/dev/null || true)
cargo_path=$(/usr/bin/readlink -f -- "$cargo_path" 2>/dev/null || true)
case "$cargo_path" in
    */.rustup/toolchains/*/bin/cargo) ;;
    *) printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2; exit 1 ;;
esac
toolchain_bin=$(/usr/bin/dirname -- "$cargo_path")
toolchain_root=$(/usr/bin/dirname -- "$toolchain_bin")
toolchains_root=$(/usr/bin/dirname -- "$toolchain_root")
rustup_root=$(/usr/bin/dirname -- "$toolchains_root")
user_root=$(/usr/bin/dirname -- "$rustup_root")
cargo_home="$user_root/.cargo"
cargo_index="$cargo_home/registry/index"
cargo_cache="$cargo_home/registry/cache"
cargo_sources="$cargo_home/registry/src"
for required in \
        "$toolchain_root" "$toolchain_root/bin/rustc" \
        "$toolchain_root/lib/rustlib/wasm32-wasip2" \
        "$cargo_index" "$cargo_cache" "$cargo_sources"; do
    if test ! -e "$required" || test -L "$required"; then
        printf '%s\n' 'error: required read-only build input is unavailable' >&2
        exit 1
    fi
done

if ! "$bwrap_path" \
        --unshare-all \
        --unshare-net \
        --die-with-parent \
        --new-session \
        --clearenv \
        --ro-bind /usr /usr \
        --symlink usr/bin /bin \
        --symlink usr/lib /lib \
        --symlink usr/lib /lib64 \
        --proc /proc \
        --dev /dev \
        --tmpfs /tmp \
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
        --bind "$target_root" /output \
        --chdir /source \
        --setenv PATH /rust-toolchain/bin:/usr/bin:/bin \
        --setenv HOME /home/build \
        --setenv CARGO_HOME /cargo-home \
        --setenv RUSTUP_HOME /rustup-home \
        --setenv RUSTC /rust-toolchain/bin/rustc \
        --setenv CARGO_NET_OFFLINE true \
        --setenv CARGO_TARGET_DIR /output/target \
        --setenv CARGO_INCREMENTAL 0 \
        --setenv LC_ALL C.UTF-8 \
        --setenv LANG C.UTF-8 \
        --setenv SOURCE_DATE_EPOCH 0 \
        --setenv RUSTFLAGS '--remap-path-prefix=/source=/usr/src/overcrow' \
        /rust-toolchain/bin/cargo build \
        --manifest-path /source/Cargo.toml \
        --release \
        --target wasm32-wasip2 \
        --locked \
        --offline \
        --lib "$@" >/dev/null 2>&1; then
    printf '%s\n' 'error: sandboxed component build failed' >&2
    exit 1
fi
