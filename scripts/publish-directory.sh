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

valid_identity() {
    case "$1" in
        '' | *[!0-9:]* | :* | *: | *:*:* | *[!0-9]:* | *:*[!0-9]) return 1 ;;
        *:*) return 0 ;;
        *) return 1 ;;
    esac
}

path_identity() {
    identity_path=$1
    test -d "$identity_path" && ! test -L "$identity_path" || return 1
    identity_value=$(/usr/bin/stat -c '%d:%i' "$identity_path" 2>/dev/null) \
        || return 1
    valid_identity "$identity_value" || return 1
    printf '%s\n' "$identity_value"
}

path_is_absent() {
    test ! -e "$1" && test ! -L "$1"
}

path_has_identity() {
    observed_identity=$(path_identity "$1") || return 1
    test "$observed_identity" = "$2"
}

transaction=rollback
mutation_active=0
interrupted=0
next_wrapper_identity=''
next_tree_identity=''
prior_identity=''
public_new_identity=''
if test -e "$public" || test -L "$public"; then
    prior_identity=$(path_identity "$public") || {
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    }
fi
quarantine_root=$(
    /usr/bin/mktemp -d "$live_parent/.${live_name}-quarantine.XXXXXXXXXX"
) || {
    printf '%s\n' 'error: publication transaction setup failed' >&2
    exit 1
}
quarantine_root_identity=$(path_identity "$quarantine_root") || {
    printf '%s\n' 'error: publication transaction setup failed' >&2
    exit 1
}

# Rename into a private, unique directory before deleting. The identity check
# after rename is the ownership proof: a substituted path is restored, never
# passed to recursive removal.
remove_owned_directory() {
    remove_path=$1
    remove_expected_identity=$2
    remove_slot=$3
    remove_quarantine="$quarantine_root/$remove_slot"
    path_has_identity "$quarantine_root" "$quarantine_root_identity" \
        || return 1
    path_is_absent "$remove_quarantine" || return 1
    if /usr/bin/mv -T -- "$remove_path" "$remove_quarantine"; then
        :
    else
        :
    fi
    remove_observed_identity=$(path_identity "$remove_quarantine") || return 1
    if test "$remove_observed_identity" != "$remove_expected_identity"; then
        if path_is_absent "$remove_path" \
                && /usr/bin/mv -T -- "$remove_quarantine" "$remove_path" \
                && path_has_identity "$remove_path" "$remove_observed_identity" \
                && path_is_absent "$remove_quarantine"; then
            :
        fi
        return 1
    fi
    if ! /usr/bin/rm -rf -- "$remove_quarantine" \
            || ! path_is_absent "$remove_quarantine"; then
        return 1
    fi
    # Exact postconditions, not the move wrapper's status, decide ownership.
    return 0
}

restore_prior_publication() {
    if test -z "$prior_identity"; then
        path_is_absent "$public"
        return
    fi
    if path_has_identity "$public" "$prior_identity"; then
        path_is_absent "$previous_public"
        return
    fi
    path_is_absent "$public" || return 1
    path_has_identity "$previous_public" "$prior_identity" || return 1
    mutation_active=1
    if "$move_restore" -T -- "$previous_public" "$public"; then
        :
    else
        :
    fi
    mutation_active=0
    path_has_identity "$public" "$prior_identity" \
        && path_is_absent "$previous_public"
}

cleanup() {
    result=$?
    trap - EXIT
    # A second signal must not interrupt the bounded rollback transaction.
    trap '' HUP INT TERM
    cleanup_failed=0

    if test "$transaction" = committed; then
        if test -z "$public_new_identity" \
                || ! path_has_identity "$public" "$public_new_identity"; then
            cleanup_failed=1
        fi
        if test -n "$next_wrapper_identity" \
                && ! remove_owned_directory "$next_public" \
                    "$next_wrapper_identity" next; then
            cleanup_failed=1
        fi
        if test -n "$prior_identity"; then
            if test "$cleanup_failed" -ne 0 \
                    || ! remove_owned_directory "$previous_public" \
                        "$prior_identity" previous; then
                cleanup_failed=1
            fi
        fi
    else
        if test -n "$next_wrapper_identity" \
                && ! remove_owned_directory "$next_public" \
                    "$next_wrapper_identity" next; then
            cleanup_failed=1
        fi
        if test -n "$public_new_identity" \
                && ! remove_owned_directory "$public" \
                    "$public_new_identity" public; then
            cleanup_failed=1
        fi
        if ! restore_prior_publication; then
            printf '%s\n' 'error: prior publication retained for recovery' >&2
            cleanup_failed=1
        fi
    fi

    if path_has_identity "$quarantine_root" "$quarantine_root_identity"; then
        if ! /usr/bin/rmdir -- "$quarantine_root"; then
            cleanup_failed=1
        fi
    else
        cleanup_failed=1
    fi
    if test "$cleanup_failed" -ne 0; then
        result=1
        printf '%s\n' 'error: publication cleanup failed' >&2
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
    next_wrapper_identity=$(path_identity "$next_public") || {
        mutation_active=0
        printf '%s\n' 'error: next publication reservation failed' >&2
        exit 1
    }
    mutation_active=0
    abort_if_interrupted
else
    mutation_active=0
    abort_if_interrupted
    printf '%s\n' 'error: next publication reservation failed' >&2
    exit 1
fi

next_tree="$next_public/tree"
staged_identity=$(path_identity "$staged_public") || {
    printf '%s\n' 'error: staged publication verification failed' >&2
    exit 1
}
move_staged_status=0
mutation_active=1
if "$move_staged" -T -- "$staged_public" "$next_tree"; then
    :
else
    move_staged_status=$?
fi
if path_is_absent "$staged_public" \
        && path_has_identity "$next_public" "$next_wrapper_identity" \
        && path_has_identity "$next_tree" "$staged_identity"; then
    next_tree_identity=$staged_identity
fi
mutation_active=0
abort_if_interrupted
if test "$move_staged_status" -ne 0; then
    printf '%s\n' 'error: staged publication move failed' >&2
    exit 1
fi
if test -z "$next_tree_identity"; then
    printf '%s\n' 'error: staged publication move was incomplete' >&2
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
    if ! path_has_identity "$public" "$prior_identity"; then
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    fi
    if ! path_has_identity "$next_tree" "$next_tree_identity"; then
        printf '%s\n' 'error: staged publication verification failed' >&2
        exit 1
    fi
    move_old_status=0
    mutation_active=1
    if "$move_old" -T -- "$public" "$previous_public"; then
        :
    else
        move_old_status=$?
    fi
    prior_moved=0
    if path_is_absent "$public" \
            && path_has_identity "$previous_public" "$prior_identity"; then
        prior_moved=1
    fi
    mutation_active=0
    abort_if_interrupted
    if test "$move_old_status" -ne 0; then
        printf '%s\n' 'error: prior publication move failed' >&2
        exit 1
    fi
    if test "$prior_moved" -ne 1; then
        printf '%s\n' 'error: prior publication move was incomplete' >&2
        exit 1
    fi
fi

if ! path_has_identity "$next_public" "$next_wrapper_identity" \
        || ! path_has_identity "$next_tree" "$next_tree_identity"; then
    printf '%s\n' 'error: staged publication verification failed' >&2
    exit 1
fi

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
public_identity=$(path_identity "$public" 2>/dev/null || :)
moved_exact=0
if test "$public_identity" = "$next_tree_identity" \
        && test ! -e "$next_tree" && test ! -L "$next_tree"; then
    public_new_identity=$next_tree_identity
    moved_exact=1
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
if test -n "$verify_tool" \
        && ! "$verify_tool" verify-tree-ledger --tree "$public" \
            --ledger "$ledger" --sha256 "$ledger_sha256" >/dev/null 2>&1; then
    printf '%s\n' 'error: published tree verification failed' >&2
    exit 1
fi
if ! path_has_identity "$public" "$public_new_identity" \
        || ! path_has_identity "$next_public" "$next_wrapper_identity" \
        || ! path_is_absent "$next_tree"; then
    printf '%s\n' 'error: published tree verification failed' >&2
    exit 1
fi
if test -n "$prior_identity" \
        && ! path_has_identity "$previous_public" "$prior_identity"; then
    printf '%s\n' 'error: published tree verification failed' >&2
    exit 1
fi
transaction=committed
