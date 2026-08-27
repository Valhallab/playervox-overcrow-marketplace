#!/bin/sh
set -eu

if test "$#" -ne 6; then
    printf '%s\n' \
        'usage: publish-directory.sh NEXT PUBLIC PREVIOUS MOVE-OLD MOVE-NEXT MOVE-RESTORE' >&2
    exit 2
fi

next_public=$1
public=$2
previous_public=$3
move_old=$4
move_next=$5
move_restore=$6

case "$next_public" in
    */.public-next.*) ;;
    *) printf '%s\n' 'error: unsafe next publication path' >&2; exit 1 ;;
esac
case "$public" in
    */public) ;;
    *) printf '%s\n' 'error: unsafe public path' >&2; exit 1 ;;
esac
case "$previous_public" in
    */.public-previous.*) ;;
    *) printf '%s\n' 'error: unsafe previous publication path' >&2; exit 1 ;;
esac
if test "$(dirname -- "$next_public")" != "$(dirname -- "$public")" \
        || test "$(dirname -- "$previous_public")" != "$(dirname -- "$public")" \
        || test ! -d "$next_public" || test -L "$next_public" \
        || test -e "$previous_public" || test -L "$previous_public"; then
    printf '%s\n' 'error: unsafe publication directories' >&2
    exit 1
fi
for operation in "$move_old" "$move_next" "$move_restore"; do
    if test ! -x "$operation"; then
        printf '%s\n' 'error: publication move operation is unavailable' >&2
        exit 1
    fi
done

had_previous=0
published=0
cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if test "$published" -eq 0; then
        if ! /usr/bin/rm -rf -- "$next_public"; then
            result=1
        fi
        if test "$had_previous" -eq 1 && ! test -e "$public" && ! test -L "$public"; then
            if ! "$move_restore" -- "$previous_public" "$public"; then
                printf '%s\n' 'error: prior publication retained at recovery path' >&2
                result=1
            fi
        fi
    elif test "$had_previous" -eq 1; then
        if ! /usr/bin/rm -rf -- "$previous_public"; then
            result=1
        fi
    fi
    exit "$result"
}
trap cleanup EXIT HUP INT TERM

if test -e "$public" || test -L "$public"; then
    if test ! -d "$public" || test -L "$public"; then
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    fi
    "$move_old" -- "$public" "$previous_public"
    had_previous=1
fi

if ! "$move_next" -- "$next_public" "$public"; then
    printf '%s\n' 'error: publication move failed' >&2
    exit 1
fi
published=1
