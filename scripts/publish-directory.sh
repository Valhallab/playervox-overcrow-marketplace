#!/bin/sh
set -eu

if test "$#" -ne 9 && test "$#" -ne 12; then
    printf '%s\n' \
        'usage: publish-directory.sh STAGED PUBLIC NEXT PREVIOUS HOOK-STAGED HOOK-OLD HOOK-NEXT HOOK-RESTORE RENAME-TOOL [VERIFY-TOOL LEDGER LEDGER-SHA256]' >&2
    exit 2
fi

staged_public=$1
public=$2
next_public=$3
previous_public=$4
hook_staged=$5
hook_old=$6
hook_next=$7
hook_restore=$8
trusted_tool=$9
verify_tool=''
ledger=''
ledger_sha256=''
if test "$#" -eq 12; then
    verify_tool=${10}
    ledger=${11}
    ledger_sha256=${12}
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
for hook in "$hook_staged" "$hook_old" "$hook_next" "$hook_restore"; do
    if test ! -f "$hook" || test ! -x "$hook"; then
        printf '%s\n' 'error: publication move hook is unavailable' >&2
        exit 1
    fi
done
if test ! -f "$trusted_tool" || test -L "$trusted_tool" \
        || test ! -x "$trusted_tool"; then
    printf '%s\n' 'error: publication verifier is unavailable' >&2
    exit 1
fi
if test "$#" -eq 12; then
    case "$ledger_sha256" in *[!0-9a-f]* | '') ledger_sha256='' ;; esac
    if test ! -f "$verify_tool" || test -L "$verify_tool" \
            || test ! -x "$verify_tool" \
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

trusted_rename() {
    "$trusted_tool" rename-noreplace \
        --live-root "$live_parent" \
        --staged-root "$staged_parent" \
        --public-name "$live_name" \
        --source "$1" --destination "$2" >/dev/null 2>&1
}

# Hooks only expose deterministic before/after test boundaries. The final
# filesystem operation is always the trusted atomic no-replace primitive.
transaction_rename() {
    rename_hook=$1
    rename_source=$2
    rename_destination=$3
    hook_before_status=0
    rename_status=0
    hook_after_status=0
    mutation_active=1
    "$rename_hook" before "$rename_source" "$rename_destination" \
        || hook_before_status=$?
    if test "$hook_before_status" -eq 0; then
        trusted_rename "$rename_source" "$rename_destination" \
            || rename_status=$?
        "$rename_hook" after "$rename_source" "$rename_destination" \
            "$rename_status" || hook_after_status=$?
    fi
    if test "$hook_before_status" -ne 0; then
        return "$hook_before_status"
    fi
    if test "$rename_status" -ne 0; then
        return "$rename_status"
    fi
    return "$hook_after_status"
}

transaction=rollback
mutation_active=0
interrupted=0
next_wrapper_identity=''
next_tree_identity=''
prior_identity=''
public_new_identity=''
quarantine_root=''
quarantine_root_identity=''

remove_owned_directory() {
    remove_path=$1
    remove_expected_identity=$2
    remove_slot=$3
    remove_hook=$4
    remove_quarantine="$quarantine_root/$remove_slot"
    path_has_identity "$quarantine_root" "$quarantine_root_identity" \
        || return 1
    path_has_identity "$remove_path" "$remove_expected_identity" || return 1
    path_is_absent "$remove_quarantine" || return 1
    remove_status=0
    transaction_rename "$remove_hook" "$remove_path" "$remove_quarantine" \
        || remove_status=$?
    remove_observed_identity=$(path_identity "$remove_quarantine" 2>/dev/null || :)
    if test "$remove_observed_identity" != "$remove_expected_identity"; then
        return 1
    fi
    if test "$remove_status" -ne 0; then
        if path_is_absent "$remove_path"; then
            trusted_rename "$remove_quarantine" "$remove_path" >/dev/null 2>&1 || :
        fi
        return 1
    fi
    if ! /usr/bin/rm -rf -- "$remove_quarantine" >/dev/null 2>&1 \
            || ! path_is_absent "$remove_quarantine"; then
        return 1
    fi
}

next_wrapper_is_owned() {
    path_has_identity "$next_public" "$next_wrapper_identity" || return 1
    if path_is_absent "$next_public/tree"; then
        return 0
    fi
    test -n "$next_tree_identity" \
        && path_has_identity "$next_public/tree" "$next_tree_identity"
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
    restore_status=0
    transaction_rename "$hook_restore" "$previous_public" "$public" \
        || restore_status=$?
    path_has_identity "$public" "$prior_identity" \
        && path_is_absent "$previous_public" \
        && test "$restore_status" -eq 0
}

cleanup() {
    result=$?
    trap - EXIT
    # Signals remain ignored until the bounded rollback has finished.
    trap '' HUP INT TERM
    cleanup_failed=0

    if test "$transaction" = rollback; then
        if test -n "$next_wrapper_identity"; then
            if next_wrapper_is_owned; then
                if ! remove_owned_directory "$next_public" \
                        "$next_wrapper_identity" rollback-next.0 /usr/bin/true; then
                    cleanup_failed=1
                else
                    next_wrapper_identity=''
                    next_tree_identity=''
                fi
            else
                cleanup_failed=1
            fi
        fi
        if test -n "$public_new_identity"; then
            if path_has_identity "$public" "$public_new_identity"; then
                if ! remove_owned_directory "$public" \
                        "$public_new_identity" rollback-public.0 /usr/bin/true; then
                    cleanup_failed=1
                else
                    public_new_identity=''
                fi
            elif ! path_is_absent "$public"; then
                cleanup_failed=1
            fi
        fi
        if ! restore_prior_publication; then
            printf '%s\n' 'error: prior publication retained for recovery' >&2
            cleanup_failed=1
        fi
    elif test -z "$public_new_identity" \
            || ! path_has_identity "$public" "$public_new_identity"; then
        cleanup_failed=1
    fi

    if test -n "$quarantine_root"; then
        if test -n "$quarantine_root_identity" \
                && path_has_identity "$quarantine_root" \
                    "$quarantine_root_identity"; then
            if ! /usr/bin/rmdir -- "$quarantine_root" >/dev/null 2>&1; then
                cleanup_failed=1
            fi
        else
            cleanup_failed=1
        fi
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

# State and traps precede the first owned transient directory.
trap cleanup EXIT
trap 'handle_signal 129' HUP
trap 'handle_signal 130' INT
trap 'handle_signal 143' TERM

if test -e "$public" || test -L "$public"; then
    prior_identity=$(path_identity "$public") || {
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    }
fi
mutation_active=1
quarantine_root=$(
    /usr/bin/mktemp -d "$live_parent/.${live_name}-quarantine.XXXXXXXXXX"
) || {
    mutation_active=0
    printf '%s\n' 'error: publication transaction setup failed' >&2
    exit 1
}
quarantine_root_identity=$(path_identity "$quarantine_root") || {
    if test -d "$quarantine_root" && test ! -L "$quarantine_root" \
            && test "$(/usr/bin/stat -c '%u:%a:%h' \
                "$quarantine_root" 2>/dev/null || :)" \
                = "$(/usr/bin/id -u):700:1" \
            && /usr/bin/rmdir -- "$quarantine_root" >/dev/null 2>&1; then
        quarantine_root=''
    fi
    mutation_active=0
    printf '%s\n' 'error: publication transaction setup failed' >&2
    exit 1
}
mutation_active=0
abort_if_interrupted

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
transaction_rename "$hook_staged" "$staged_public" "$next_tree" \
    || move_staged_status=$?
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

if test "$#" -eq 12 \
        && ! "$verify_tool" verify-tree-ledger --tree "$next_tree" \
            --ledger "$ledger" --sha256 "$ledger_sha256" >/dev/null 2>&1; then
    printf '%s\n' 'error: staged publication verification failed' >&2
    exit 1
fi

if test -n "$prior_identity"; then
    if ! path_has_identity "$public" "$prior_identity" \
            || ! path_has_identity "$next_tree" "$next_tree_identity"; then
        printf '%s\n' 'error: existing public path is unsafe' >&2
        exit 1
    fi
    move_old_status=0
    transaction_rename "$hook_old" "$public" "$previous_public" \
        || move_old_status=$?
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

move_next_status=0
transaction_rename "$hook_next" "$next_tree" "$public" \
    || move_next_status=$?
public_identity=$(path_identity "$public" 2>/dev/null || :)
moved_exact=0
if test "$public_identity" = "$next_tree_identity" \
        && path_is_absent "$next_tree"; then
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
if test "$#" -eq 12 \
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

# Finalization is still rollback-capable until the prior tree has been removed.
mutation_active=1
if ! remove_owned_directory "$next_public" "$next_wrapper_identity" \
        finalize-next.0 "$hook_next"; then
    mutation_active=0
    printf '%s\n' 'error: publication cleanup failed' >&2
    exit 1
fi
next_wrapper_identity=''
next_tree_identity=''
if test -n "$prior_identity"; then
    if ! path_has_identity "$public" "$public_new_identity" \
            || ! path_has_identity "$previous_public" "$prior_identity" \
            || ! remove_owned_directory "$previous_public" "$prior_identity" \
                finalize-previous.0 /usr/bin/true; then
        mutation_active=0
        printf '%s\n' 'error: publication cleanup failed' >&2
        exit 1
    fi
fi
transaction=committed
mutation_active=0
abort_if_interrupted
