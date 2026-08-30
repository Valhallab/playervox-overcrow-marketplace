#!/bin/sh
set -eu
umask 077

if test "$#" -ne 1; then
    printf '%s\n' 'usage: resolve-pinned-rust.sh ABSOLUTE-SOURCE-ROOT' >&2
    exit 2
fi
source_root=$1
case "$source_root" in
    /*) ;;
    *) printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2; exit 1 ;;
esac

invoking_uid=$(/usr/bin/id -u)
pin_file="$source_root/rust-toolchain.toml"
if test ! -f "$pin_file" || test -L "$pin_file" \
        || test "$(/usr/bin/stat -c '%u:%h' "$pin_file")" != "$invoking_uid:1" \
        || test "$(/usr/bin/stat -c '%s' "$pin_file")" -gt 4096 \
        || /usr/bin/find "$pin_file" -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
if ! /usr/bin/awk '
        /^[[:space:]]*\[/ {
            in_toolchain = ($0 ~ /^[[:space:]]*\[toolchain\][[:space:]]*$/)
            next
        }
        /^[[:space:]]*channel[[:space:]]*=/ {
            count += 1
            if (!in_toolchain || $0 !~ /^[[:space:]]*channel[[:space:]]*=[[:space:]]*"1[.]98[.]0"[[:space:]]*$/) exit 1
        }
        END { if (count != 1) exit 1 }
    ' "$pin_file"; then
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

toolchain_root="$user_root/.rustup/toolchains/1.98.0-$rust_host"
cargo_path="$toolchain_root/bin/cargo"
rustc_path="$toolchain_root/bin/rustc"
cargo_index="$user_root/.cargo/registry/index"
cargo_cache="$user_root/.cargo/registry/cache"
cargo_sources="$user_root/.cargo/registry/src"
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
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$binary")" \
                != "$invoking_uid:755:1"; then
        printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
        exit 1
    fi
done

version_file=$(/usr/bin/mktemp /tmp/marketplace-rustc-version.XXXXXXXXXX) || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$version_file"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
if ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        /usr/bin/timeout --signal=KILL 5 \
        /usr/bin/prlimit --cpu=5 --as=1073741824 --nofile=64 --fsize=4096 -- \
        "$rustc_path" --version --verbose >"$version_file" 2>/dev/null \
        || test "$(/usr/bin/grep -c '^release: 1[.]98[.]0$' "$version_file" || :)" != 1 \
        || test "$(/usr/bin/grep -c '^rustc 1[.]98[.]0 ' "$version_file" || :)" != 1; then
    printf '%s\n' 'error: pinned Rust toolchain is unavailable' >&2
    exit 1
fi
printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$toolchain_root" "$cargo_path" "$rustc_path" \
    "$cargo_index" "$cargo_cache" "$cargo_sources"
