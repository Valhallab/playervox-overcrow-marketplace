#!/bin/sh
set -eu

if test "$#" -ne 8; then
    printf '%s\n' \
        'usage: publish-directory.sh STAGED PUBLIC NEXT PREVIOUS MOVE-STAGED MOVE-OLD MOVE-NEXT MOVE-RESTORE' >&2
    exit 2
fi

staged_public=$1
public=$2
next_public=$3
previous_public=$4
move_staged=$5
move_old=$6
move_next=$7
move_restore=$8

case "$staged_public" in
    /*/public) ;;
    *) printf '%s\n' 'error: unsafe staged publication path' >&2; exit 1 ;;
esac
case "$public" in
    /*/public) ;;
    *) printf '%s\n' 'error: unsafe public path' >&2; exit 1 ;;
esac

live_parent=$(/usr/bin/dirname -- "$public")
staged_parent=$(/usr/bin/dirname -- "$staged_public")
case "$next_public" in
    "$live_parent"/.public-next.?*) ;;
    *) printf '%s\n' 'error: unsafe next publication path' >&2; exit 1 ;;
esac
case "$previous_public" in
    "$live_parent"/.public-previous.?*) ;;
    *) printf '%s\n' 'error: unsafe previous publication path' >&2; exit 1 ;;
esac

if test "$staged_parent" = "$live_parent" \
        || test "$(/usr/bin/dirname -- "$next_public")" != "$live_parent" \
        || test "$(/usr/bin/dirname -- "$previous_public")" != "$live_parent" \
        || test ! -d "$live_parent" || test -L "$live_parent" \
        || test ! -d "$staged_parent" || test -L "$staged_parent" \
        || test ! -d "$staged_public" || test -L "$staged_public" \
        || test -e "$next_public" || test -L "$next_public" \
        || test -e "$previous_public" || test -L "$previous_public"; then
    printf '%s\n' 'error: unsafe publication directories' >&2
    exit 1
fi
if test -e "$public" || test -L "$public"; then
    if test ! -d "$public" || test -L "$public"; then
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    fi
fi
for operation in "$move_staged" "$move_old" "$move_next" "$move_restore"; do
    if test ! -f "$operation" || test ! -x "$operation"; then
        printf '%s\n' 'error: publication move operation is unavailable' >&2
        exit 1
    fi
done

next_owned=0
prior_at_recovery=0
published=0
mutation_active=0
interrupted=0

cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if test "$next_owned" -eq 1 \
            && { test -e "$next_public" || test -L "$next_public"; }; then
        if ! /usr/bin/rm -rf -- "$next_public"; then
            result=1
        fi
    fi
    if test "$published" -eq 0 && test "$prior_at_recovery" -eq 1; then
        if test ! -e "$public" && test ! -L "$public"; then
            if test -d "$previous_public" && ! test -L "$previous_public" \
                    && "$move_restore" -T -- "$previous_public" "$public"; then
                prior_at_recovery=0
            else
                printf '%s\n' 'error: prior publication retained at recovery path' >&2
                result=1
            fi
        fi
    elif test "$published" -eq 1 && test "$prior_at_recovery" -eq 1; then
        if ! /usr/bin/rm -rf -- "$previous_public"; then
            result=1
        fi
    fi
    exit "$result"
}

handle_signal() {
    interrupted=$1
    if test "$mutation_active" -eq 0; then
        exit "$interrupted"
    fi
}

abort_if_interrupted() {
    if test "$interrupted" -ne 0; then
        exit "$interrupted"
    fi
}

trap cleanup EXIT
trap 'handle_signal 129' HUP
trap 'handle_signal 130' INT
trap 'handle_signal 143' TERM

mutation_active=1
if /usr/bin/mkdir -m 0700 -- "$next_public"; then
    next_owned=1
    mutation_active=0
    abort_if_interrupted
else
    mutation_active=0
    abort_if_interrupted
    printf '%s\n' 'error: next publication reservation failed' >&2
    exit 1
fi

next_tree="$next_public/tree"
mutation_active=1
if "$move_staged" -T -- "$staged_public" "$next_tree"; then
    mutation_active=0
    abort_if_interrupted
    if test -e "$staged_public" || test -L "$staged_public" \
            || test ! -d "$next_tree" || test -L "$next_tree"; then
        printf '%s\n' 'error: staged publication move was incomplete' >&2
        exit 1
    fi
else
    mutation_active=0
    abort_if_interrupted
    printf '%s\n' 'error: staged publication move failed' >&2
    exit 1
fi

if test -e "$public" || test -L "$public"; then
    if test ! -d "$public" || test -L "$public"; then
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    fi
    prior_at_recovery=1
    mutation_active=1
    if "$move_old" -T -- "$public" "$previous_public"; then
        mutation_active=0
        abort_if_interrupted
        if test -e "$public" || test -L "$public" \
                || test ! -d "$previous_public" || test -L "$previous_public"; then
            printf '%s\n' 'error: prior publication move was incomplete' >&2
            exit 1
        fi
    else
        mutation_active=0
        abort_if_interrupted
        printf '%s\n' 'error: prior publication move failed' >&2
        exit 1
    fi
fi

mutation_active=1
if "$move_next" -T -- "$next_tree" "$public"; then
    mutation_active=0
    abort_if_interrupted
    if test -e "$next_tree" || test -L "$next_tree" \
            || test ! -d "$public" || test -L "$public"; then
        printf '%s\n' 'error: publication move was incomplete' >&2
        exit 1
    fi
    published=1
else
    mutation_active=0
    abort_if_interrupted
    printf '%s\n' 'error: publication move failed' >&2
    exit 1
fi
