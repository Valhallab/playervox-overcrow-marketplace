#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: resolve-system-gcc.sh' >&2
    exit 2
fi

fail() {
    printf '%s\n' 'error: trusted system compiler is unavailable' >&2
    exit 1
}

for directory in /usr /usr/bin; do
    if test ! -d "$directory" || test -L "$directory" \
            || test "$(/usr/bin/stat -c '%u:%a' "$directory")" != 0:755; then
        fail
    fi
done

# Ubuntu exposes gcc through a distribution-managed link to a versioned binary.
system_gcc=$(/usr/bin/readlink -e -- /usr/bin/gcc 2>/dev/null) || fail
case "$system_gcc" in
    /usr/bin/*) ;;
    *) fail ;;
esac
compiler_name=${system_gcc#/usr/bin/}
case "$compiler_name" in
    '' | */* | *[!A-Za-z0-9._+-]*) fail ;;
esac
case "$compiler_name" in
    gcc | gcc-[0-9]* | *-linux-gnu-gcc-[0-9]*) ;;
    *) fail ;;
esac
if test ! -f "$system_gcc" || test -L "$system_gcc" \
        || test "$(/usr/bin/stat -c '%u:%a' "$system_gcc")" != 0:755; then
    fail
fi

printf '%s\n' "$system_gcc"
