#!/bin/sh
set -eu

if test "$#" -ne 8 && test "$#" -ne 11; then
    printf '%s\n' \
        'usage: publish-directory.sh STAGED PUBLIC NEXT PREVIOUS MOVE-STAGED MOVE-OLD MOVE-NEXT MOVE-RESTORE [VERIFY-TOOL LEDGER LEDGER-SHA256]' >&2
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
verify_tool=''
ledger=''
ledger_sha256=''
if test "$#" -eq 11; then
    verify_tool=$9
    ledger=${10}
    ledger_sha256=${11}
fi

case "$staged_public" in
    /*/public) ;;
    *) printf '%s\n' 'error: unsafe staged publication path' >&2; exit 1 ;;
esac
case "$public" in
    /*/public | /*/published) ;;
    *) printf '%s\n' 'error: unsafe public path' >&2; exit 1 ;;
esac

live_parent=$(/usr/bin/dirname -- "$public")
staged_parent=$(/usr/bin/dirname -- "$staged_public")
live_name=${public##*/}
case "$next_public" in
    "$live_parent"/."$live_name"-next.?*) ;;
    *) printf '%s\n' 'error: unsafe next publication path' >&2; exit 1 ;;
esac
case "$previous_public" in
    "$live_parent"/."$live_name"-previous.?*) ;;
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
if test -n "$verify_tool"; then
    case "$ledger_sha256" in *[!0-9a-f]* | '') ledger_sha256='' ;; esac
    if test ! -f "$verify_tool" || test -L "$verify_tool" || test ! -x "$verify_tool" \
            || test ! -f "$ledger" || test -L "$ledger" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$ledger" 2>/dev/null || :)" \
                != "$(/usr/bin/id -u):600:1" \
            || test "${#ledger_sha256}" -ne 64; then
        printf '%s\n' 'error: publication verifier is unavailable' >&2
        exit 1
    fi
fi

next_owned=0
prior_at_recovery=0
published=0
mutation_active=0
interrupted=0
new_at_public=0

cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if test "$next_owned" -eq 1 \
            && { test -e "$next_public" || test -L "$next_public"; }; then
        if ! /usr/bin/rm -rf -- "$next_public"; then
            result=1
        fi
    fi
    if test "$published" -eq 0 && test "$new_at_public" -eq 1 \
            && { test -e "$public" || test -L "$public"; }; then
        if /usr/bin/rm -rf -- "$public"; then
            new_at_public=0
        else
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

if test -n "$verify_tool" \
        && ! "$verify_tool" verify-tree-ledger --tree "$next_tree" \
            --ledger "$ledger" --sha256 "$ledger_sha256" >/dev/null 2>&1; then
    printf '%s\n' 'error: staged publication verification failed' >&2
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

next_tree_identity=$(/usr/bin/stat -c '%d:%i' "$next_tree" 2>/dev/null || :)
case "$next_tree_identity" in
    *[!0-9:]* | :* | *: | *:*:*)
        printf '%s\n' 'error: staged publication verification failed' >&2
        exit 1
        ;;
esac

# A move helper can rename successfully and still report failure, and a signal
# can arrive after rename(2) but before its wrapper returns. Keep signal exit
# deferred until the exact postconditions establish whether our inode reached
# the public name, so cleanup never mistakes the new tree for the prior tree.
move_next_status=0
mutation_active=1
if "$move_next" -T -- "$next_tree" "$public"; then
    :
else
    move_next_status=$?
fi
public_identity=''
if test -d "$public" && test ! -L "$public"; then
    public_identity=$(/usr/bin/stat -c '%d:%i' "$public" 2>/dev/null || :)
fi
next_identity=''
if test -d "$next_tree" && test ! -L "$next_tree"; then
    next_identity=$(/usr/bin/stat -c '%d:%i' "$next_tree" 2>/dev/null || :)
fi
if test "$public_identity" = "$next_tree_identity"; then
    new_at_public=1
fi
moved_exact=0
unmoved_exact=0
if test "$new_at_public" -eq 1 \
        && test ! -e "$next_tree" && test ! -L "$next_tree"; then
    moved_exact=1
elif test -z "$public_identity" \
        && test ! -e "$public" && test ! -L "$public" \
        && test "$next_identity" = "$next_tree_identity"; then
    unmoved_exact=1
fi
mutation_active=0
abort_if_interrupted

if test "$move_next_status" -ne 0; then
    printf '%s\n' 'error: publication move failed' >&2
    exit 1
fi
if test "$moved_exact" -ne 1; then
    printf '%s\n' 'error: publication move was incomplete' >&2
    exit 1
fi
if test "$unmoved_exact" -eq 1; then
    printf '%s\n' 'error: publication move was incomplete' >&2
    exit 1
fi
if test -n "$verify_tool" \
        && ! "$verify_tool" verify-tree-ledger --tree "$public" \
            --ledger "$ledger" --sha256 "$ledger_sha256" >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree verification failed' >&2
    exit 1
fi
published=1
new_at_public=0
