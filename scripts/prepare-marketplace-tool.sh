#!/bin/sh
set -eu
umask 077

if test "$#" -ne 2; then
    printf '%s\n' 'usage: prepare-marketplace-tool.sh ABSOLUTE-SOURCE-ROOT ABSOLUTE-PRIVATE-WORK' >&2
    exit 2
fi
source_root=$1
private_work=$2
case "$source_root:$private_work" in
    /*:/*) ;;
    *) printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2; exit 1 ;;
esac
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
case "$script_dir" in
    "$source_root"/scripts) ;;
    *) printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2; exit 1 ;;
esac
if test ! -d "$source_root" || test -L "$source_root" \
        || test "$(CDPATH='' cd -- "$source_root" && pwd -P)" != "$source_root" \
        || test ! -d "$private_work" || test -L "$private_work" \
        || test "$(CDPATH='' cd -- "$private_work" && pwd -P)" != "$private_work" \
        || test "$(/usr/bin/stat -c '%u:%a' "$private_work")" \
            != "$(/usr/bin/id -u):700"; then
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
fi

resolved_toolchain=$(sh "$script_dir/resolve-pinned-rust.sh" "$source_root") || {
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
}
tab=$(printf '\t')
IFS="$tab" read -r toolchain_root cargo_path rustc_path \
    cargo_index cargo_cache cargo_sources <<EOF
$resolved_toolchain
EOF
test -n "$cargo_sources" || {
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
}

tool_home="$private_work/home"
tool_cargo_home="$private_work/cargo-home"
tool_rustup_home="$private_work/rustup-home"
tool_target="$private_work/target"
if ! /usr/bin/install -d -m 0700 \
        "$tool_home" "$tool_cargo_home/registry" "$tool_rustup_home" "$tool_target" \
        || ! /usr/bin/ln -s -- "$cargo_index" "$tool_cargo_home/registry/index" \
        || ! /usr/bin/ln -s -- "$cargo_cache" "$tool_cargo_home/registry/cache" \
        || ! /usr/bin/ln -s -- "$cargo_sources" "$tool_cargo_home/registry/src"; then
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
fi
if ! (CDPATH='' cd / && \
        /usr/bin/env -i \
            PATH="$toolchain_root/bin:/usr/bin:/bin" \
            HOME="$tool_home" CARGO_HOME="$tool_cargo_home" \
            RUSTUP_HOME="$tool_rustup_home" RUSTC="$rustc_path" \
            CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
            CARGO_TARGET_DIR="$tool_target" LC_ALL=C.UTF-8 LANG=C.UTF-8 \
            /usr/bin/timeout --signal=TERM --kill-after=5 180 \
            /usr/bin/prlimit --cpu=120 --as=4294967296 --nproc=4096 \
                --nofile=256 --fsize=268435456 -- \
            "$cargo_path" build \
                --manifest-path "$source_root/tools/marketplace-tool/Cargo.toml" \
                --package marketplace-tool --release --locked --offline --quiet); then
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
fi
built_tool="$tool_target/release/marketplace-tool"
trusted_tool="$private_work/marketplace-tool"
if test ! -f "$built_tool" || test -L "$built_tool" \
        || ! /usr/bin/install -m 0700 -- "$built_tool" "$trusted_tool" \
        || test ! -f "$trusted_tool" || test -L "$trusted_tool" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$trusted_tool")" \
            != "$(/usr/bin/id -u):700:1"; then
    printf '%s\n' 'error: trusted marketplace tool is unavailable' >&2
    exit 1
fi
printf '%s\n' "$trusted_tool"
