#!/bin/sh
set -eu

if test "$#" -gt 1; then
    printf '%s\n' \
        'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|empty-next|empty-previous|empty-public|empty-restore|quarantine-collision|identity-race|foreign-next|foreign-public|foreign-previous|cleanup-signal|restore-signal|rollback|final-move-fail|final-move-signal|verified-race|publish-noop|restore-failure|retirement-failure]' >&2
    exit 2
fi
selected_case=${1:-all}
case "$selected_case" in
    all | validation | signal | post-move | race-next | race-previous \
        | empty-next | empty-previous | empty-public | empty-restore \
        | quarantine-collision \
        | identity-race | foreign-next | foreign-public | foreign-previous \
        | cleanup-signal | restore-signal | rollback | final-move-fail \
        | final-move-signal | verified-race | publish-noop | restore-failure \
        | retirement-failure) ;;
    *)
        printf '%s\n' \
            'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|empty-next|empty-previous|empty-public|empty-restore|quarantine-collision|identity-race|foreign-next|foreign-public|foreign-previous|cleanup-signal|restore-signal|rollback|final-move-fail|final-move-signal|verified-race|publish-noop|restore-failure|retirement-failure]' >&2
        exit 2
        ;;
esac

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-rollback.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM
git clone --quiet --no-hardlinks "$repo_root" "$scratch/repository"
copy="$scratch/repository"
public="$copy/public"
helper_source="$repo_root/scripts/publish-directory.sh"
tool_work="$scratch/tool"
/usr/bin/mkdir -m 0700 -- "$tool_work"
trusted_tool=$(sh "$repo_root/scripts/prepare-marketplace-tool.sh" \
    "$repo_root" "$tool_work")
helper="$scratch/publish-wrapper"
export PUBLISH_HELPER_SOURCE="$helper_source" PUBLISH_RENAME_TOOL="$trusted_tool"
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'case "$5" in /usr/bin/mv) hook_staged=/usr/bin/true ;; *) hook_staged=$5 ;; esac' \
    'case "$6" in /usr/bin/mv) hook_old=/usr/bin/true ;; *) hook_old=$6 ;; esac' \
    'case "$7" in /usr/bin/mv) hook_next=/usr/bin/true ;; *) hook_next=$7 ;; esac' \
    'case "$8" in /usr/bin/mv) hook_restore=/usr/bin/true ;; *) hook_restore=$8 ;; esac' \
    'if test "$#" -eq 8; then' \
    '    exec sh "$PUBLISH_HELPER_SOURCE" "$1" "$2" "$3" "$4" "$hook_staged" "$hook_old" "$hook_next" "$hook_restore" "$PUBLISH_RENAME_TOOL"' \
    'fi' \
    'test "$#" -eq 11' \
    'exec sh "$PUBLISH_HELPER_SOURCE" "$1" "$2" "$3" "$4" "$hook_staged" "$hook_old" "$hook_next" "$hook_restore" "$PUBLISH_RENAME_TOOL" "$9" "${10}" "${11}"' \
    >"$helper"
/usr/bin/chmod 0700 "$helper"
tracked_paths="$scratch/tracked-paths"
tracked_before="$scratch/tracked-before"
tracked_after="$scratch/tracked-after"
git -C "$copy" ls-files '*manifest.json' marketplace/development-catalog-state.json \
    >"$tracked_paths"

snapshot_tracked() {
    destination=$1
    (
        cd "$copy"
        while IFS= read -r path; do
            /usr/bin/sha256sum "$path"
        done <"$tracked_paths"
    ) >"$destination"
}

make_staged_public() {
    staged_public=$1
    /usr/bin/mkdir -p "$staged_public/nested"
    printf '%s\n' next >"$staged_public/next"
    printf '%s\n' 'nested next bytes' >"$staged_public/nested/file with spaces"
}

make_foreign_tree() {
    foreign_root=$1
    foreign_label=$2
    /usr/bin/mkdir -p "$foreign_root/nested"
    printf '%s\n' "$foreign_label" >"$foreign_root/foreign.txt"
    printf '%s\n' 'foreign nested bytes' >"$foreign_root/nested/data"
    /usr/bin/ln -s -- foreign.txt "$foreign_root/link"
    /usr/bin/chmod 0711 "$foreign_root"
    /usr/bin/chmod 0700 "$foreign_root/nested"
    /usr/bin/chmod 0640 "$foreign_root/foreign.txt" "$foreign_root/nested/data"
}

snapshot_tree() {
    root=$1
    destination=$2
    unsorted="$destination.unsorted"
    : >"$unsorted"
    /usr/bin/find "$root" -xdev -type d \
        -printf 'directory\t%P\t%U\t%G\t%m\t%n\n' >>"$unsorted"
    /usr/bin/find "$root" -xdev -type f -printf '%P\n' \
        | LC_ALL=C /usr/bin/sort \
        | while IFS= read -r relative; do
            metadata=$(/usr/bin/stat -c '%u\t%g\t%a\t%h\t%s' "$root/$relative")
            digest=$(/usr/bin/sha256sum "$root/$relative" | /usr/bin/cut -d ' ' -f 1)
            printf 'file\t%s\t%b\t%s\n' "$relative" "$metadata" "$digest"
        done >>"$unsorted"
    /usr/bin/find "$root" -xdev ! -type d ! -type f \
        -printf 'other\t%P\t%y\t%U\t%G\t%m\t%n\t%l\n' >>"$unsorted"
    LC_ALL=C /usr/bin/sort "$unsorted" >"$destination"
    /usr/bin/rm -f -- "$unsorted"
}

assert_prior_public() {
    test ! -L "$public"
    test "$(CDPATH='' cd -- "$public" && pwd -P)" = "$public"
    actual="$scratch/actual-public.manifest"
    snapshot_tree "$public" "$actual"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" "$actual"
    /usr/bin/rm -f -- "$actual"
}

assert_absent() {
    path=$1
    test ! -e "$path" && test ! -L "$path"
}

assert_recorded_identity() {
    identity_path=$1
    identity_record=$2
    test -d "$identity_path" && test ! -L "$identity_path"
    test "$(/usr/bin/stat -c '%d:%i' "$identity_path")" \
        = "$(/usr/bin/cat "$identity_record")"
}

should_run() {
    test "$selected_case" = all || test "$selected_case" = "$1"
}

/usr/bin/mkdir -p "$public/nested"
printf '%s\n' prior >"$public/prior"
printf '%s\n' 'nested prior bytes' >"$public/nested/file with spaces"
/usr/bin/cp -a -- "$public" "$scratch/prior-public"
snapshot_tree "$scratch/prior-public" "$scratch/prior-public.manifest"
snapshot_tracked "$tracked_before"

move_then_signal="$scratch/move-then-signal"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    '/usr/bin/kill -TERM "$PPID"' >"$move_then_signal"
/usr/bin/chmod 0700 "$move_then_signal"

move_then_fail="$scratch/move-then-fail"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    '/usr/bin/false' >"$move_then_fail"
/usr/bin/chmod 0700 "$move_then_fail"

move_into_raced_destination="$scratch/move-into-raced-destination"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = after && exit 0' \
    'destination=$3' \
    '/usr/bin/mkdir -- "$destination"' \
    'printf "%s\n" racer >"$destination/racer"' \
    >"$move_into_raced_destination"
/usr/bin/chmod 0700 "$move_into_raced_destination"

move_then_mutate="$scratch/move-then-mutate"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    'printf "%s\n" raced >"$3/raced-after-verification"' >"$move_then_mutate"
/usr/bin/chmod 0700 "$move_then_mutate"

final_tree_verifier="$scratch/final-tree-verifier"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$#" -eq 7 && test "$1" = verify-tree-ledger && test "$2" = --tree' \
    'test "$4" = --ledger && test "$6" = --sha256' \
    'test -f "$5" && test "$7" = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    'test ! -e "$3/raced-after-verification" && test ! -L "$3/raced-after-verification"' \
    >"$final_tree_verifier"
/usr/bin/chmod 0700 "$final_tree_verifier"
final_tree_ledger="$scratch/final-tree.ledger"
printf '%s\n' fixture-ledger >"$final_tree_ledger"
/usr/bin/chmod 0600 "$final_tree_ledger"

# Mutation helper for the old ordering, where ownership was claimed before this
# staged-move boundary without first reserving the wrapper.
old_move_boundary_contender="$scratch/old-move-boundary-contender"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'if /usr/bin/mkdir -m 0700 -- "$NEXT_PUBLIC_AT_MOVE_BOUNDARY" 2>/dev/null; then' \
    '    /usr/bin/mkdir -- "$NEXT_PUBLIC_AT_MOVE_BOUNDARY/foreign"' \
    '    printf "%s\n" foreign-owned >"$NEXT_PUBLIC_AT_MOVE_BOUNDARY/foreign/owned.txt"' \
    '    : >"$CONTENDER_CREATED_WRAPPER"' \
    'fi' \
    '/usr/bin/false' >"$old_move_boundary_contender"
/usr/bin/chmod 0700 "$old_move_boundary_contender"

hide_next_during_old_move="$scratch/hide-next-during-old-move"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    '/usr/bin/mv -T -- "$NEXT_TREE_TO_HIDE" "$HIDDEN_NEXT_TREE"' \
    >"$hide_next_during_old_move"
/usr/bin/chmod 0700 "$hide_next_during_old_move"

restore_hidden_then_move="$scratch/restore-hidden-then-move"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = after && exit 0' \
    ': >"$FINAL_MOVE_WAS_CALLED"' \
    '/usr/bin/mv -T -- "$HIDDEN_NEXT_TREE" "$2"' \
    >"$restore_hidden_then_move"
/usr/bin/chmod 0700 "$restore_hidden_then_move"

substitute_next_then_fail="$scratch/substitute-next-then-fail"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    '/usr/bin/mv -T -- "$NEXT_WRAPPER_TO_SUBSTITUTE" "$OWNED_NEXT_HIDDEN"' \
    '/usr/bin/mv -T -- "$FOREIGN_NEXT_SOURCE" "$NEXT_WRAPPER_TO_SUBSTITUTE"' \
    '/usr/bin/false' \
    >"$substitute_next_then_fail"
/usr/bin/chmod 0700 "$substitute_next_then_fail"

substitute_public_verifier="$scratch/substitute-public-verifier"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$#" -eq 7 && test "$1" = verify-tree-ledger && test "$2" = --tree' \
    'test "$4" = --ledger && test "$6" = --sha256' \
    'if test "$3" = "$PUBLIC_TO_SUBSTITUTE"; then' \
    '    /usr/bin/mv -T -- "$PUBLIC_TO_SUBSTITUTE" "$OWNED_PUBLIC_HIDDEN"' \
    '    /usr/bin/mv -T -- "$FOREIGN_PUBLIC_SOURCE" "$PUBLIC_TO_SUBSTITUTE"' \
    '    exit 1' \
    'fi' \
    'exit 0' \
    >"$substitute_public_verifier"
/usr/bin/chmod 0700 "$substitute_public_verifier"

substitute_previous_after_move="$scratch/substitute-previous-after-move"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    '/usr/bin/mv -T -- "$PREVIOUS_TO_SUBSTITUTE" "$OWNED_PREVIOUS_HIDDEN"' \
    '/usr/bin/mv -T -- "$FOREIGN_PREVIOUS_SOURCE" "$PREVIOUS_TO_SUBSTITUTE"' \
    >"$substitute_previous_after_move"
/usr/bin/chmod 0700 "$substitute_previous_after_move"

signal_during_restore="$scratch/signal-during-restore"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = after && exit 0' \
    'parent=$PPID' \
    '/usr/bin/kill -TERM "$parent"' \
    '/usr/bin/sleep 0.05' \
    'if /usr/bin/kill -0 "$parent" 2>/dev/null; then' \
    '    exit 0' \
    'else' \
    '    exit 143' \
    'fi' \
    >"$signal_during_restore"
/usr/bin/chmod 0700 "$signal_during_restore"

signal_during_cleanup_removal="$scratch/signal-during-cleanup-removal"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before || exit 0' \
    'case "$3" in */finalize-next.0) ;; *) exit 0 ;; esac' \
    '/usr/bin/mkdir -- "$NEXT_WRAPPER_TO_WATCH/removal-fixture"' \
    'entry=0' \
    'while test "$entry" -lt 4096; do' \
    '    : >"$NEXT_WRAPPER_TO_WATCH/removal-fixture/$entry"' \
    '    entry=$((entry + 1))' \
    'done' \
    'parent=$PPID' \
    '/usr/bin/kill -TERM "$parent"' \
    '(' \
    '    /usr/bin/sleep 0.01' \
    '    /usr/bin/kill -TERM "$parent" 2>/dev/null || :' \
    ') &' \
    '/usr/bin/false' \
    >"$signal_during_cleanup_removal"
/usr/bin/chmod 0700 "$signal_during_cleanup_removal"

move_into_empty_destination="$scratch/move-into-empty-destination"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = after && exit 0' \
    'destination=$3' \
    '/usr/bin/mkdir -- "$destination"' \
    '/usr/bin/stat -c "%d:%i" "$destination" >"$FOREIGN_IDENTITY_RECORD"' \
    >"$move_into_empty_destination"
/usr/bin/chmod 0700 "$move_into_empty_destination"

move_then_add_quarantine_collision="$scratch/move-then-add-quarantine-collision"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$1" = before && exit 0' \
    'test -f "$FOREIGN_IDENTITY_RECORD" && exit 0' \
    'quarantine=$(/usr/bin/find "$LIVE_ROOT_FOR_COLLISION" -maxdepth 1 -type d -name ".public-quarantine.*" -print -quit)' \
    'test -n "$quarantine"' \
    '/usr/bin/mkdir -- "$quarantine/finalize-next.0"' \
    '/usr/bin/stat -c "%d:%i" "$quarantine/finalize-next.0" >"$FOREIGN_IDENTITY_RECORD"' \
    >"$move_then_add_quarantine_collision"
/usr/bin/chmod 0700 "$move_then_add_quarantine_collision"

retirement_failure_tool="$scratch/retirement-failure-tool"
# shellcheck disable=SC2016 # generated helper expands fixture variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    '"$REAL_PUBLISH_RENAME_TOOL" "$@"' \
    'destination=' \
    'while test "$#" -gt 0; do' \
    '    case "$1" in' \
    '        --destination) destination=$2; shift 2 ;;' \
    '        *) shift ;;' \
    '    esac' \
    'done' \
    'case "$destination" in' \
    '    */finalize-previous.0)' \
    '        quarantine=$(/usr/bin/dirname -- "$destination")' \
    '        /usr/bin/mv -T -- "$destination" "$quarantine/retired-prior"' \
    '        /usr/bin/mkdir -m 0700 -- "$destination"' \
    '        printf "%s\n" foreign >"$destination/foreign.txt"' \
    '        ;;' \
    'esac' \
    >"$retirement_failure_tool"
/usr/bin/chmod 0700 "$retirement_failure_tool"

if should_run validation; then
    staged_public="$scratch/staged-validation/public"
    next_public="$copy/.public-next.validation"
    previous_public="$copy/.public-previous.validation"
    make_staged_public "$staged_public"
    set +e
    sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
        /missing/move /usr/bin/mv /usr/bin/mv /usr/bin/mv
    validation_result=$?
    set -e
    if test "$validation_result" -ne 1; then
        printf '%s\n' 'error: validation failure did not use the safe helper failure path' >&2
        exit 1
    fi
    test -f "$staged_public/next"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run signal; then
    staged_public="$scratch/staged-signal/public"
    next_public="$copy/.public-next.signal"
    previous_public="$copy/.public-previous.signal"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$move_then_signal" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: signal after staged-public move unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run post-move; then
    staged_public="$scratch/staged-post-move/public"
    next_public="$copy/.public-next.post-move"
    previous_public="$copy/.public-previous.post-move"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$move_then_fail" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: failed staged-public move unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run race-next; then
    staged_public="$scratch/staged-race-next/public"
    next_public="$copy/.public-next.race"
    previous_public="$copy/.public-previous.race-next"
    contender_created_wrapper="$scratch/old-move-boundary-contender-created-wrapper"
    make_staged_public "$staged_public"
    # The fixed publisher has already reserved next_public at this injected move
    # boundary. Reverting to the old ordering lets the contender create it.
    if NEXT_PUBLIC_AT_MOVE_BOUNDARY="$next_public" \
            CONTENDER_CREATED_WRAPPER="$contender_created_wrapper" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$old_move_boundary_contender" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: old move-boundary contender unexpectedly accepted publication' >&2
        exit 1
    fi
    test -f "$staged_public/next"
    foreign_marker="$next_public/foreign/owned.txt"
    if test -f "$contender_created_wrapper"; then
        test -f "$foreign_marker"
        test "$(/usr/bin/cat "$foreign_marker")" = foreign-owned
        /usr/bin/rm -rf -- "$next_public"
    else
        assert_absent "$next_public"
    fi
    assert_absent "$previous_public"
    assert_prior_public

    # Deterministically present a foreign wrapper at the fixed mkdir reservation
    # boundary and verify refusal leaves the entire wrapper unchanged.
    /usr/bin/mkdir -m 0700 -- "$next_public"
    /usr/bin/mkdir -- "$next_public/foreign"
    printf '%s\n' foreign-owned >"$foreign_marker"
    preexisting_next="$scratch/preexisting-next"
    /usr/bin/cp -a -- "$next_public" "$preexisting_next"
    snapshot_tree "$preexisting_next" "$scratch/preexisting-next.manifest"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: pre-existing next wrapper unexpectedly accepted publication' >&2
        exit 1
    fi
    test -f "$staged_public/next"
    test -f "$foreign_marker"
    test "$(/usr/bin/cat "$foreign_marker")" = foreign-owned
    snapshot_tree "$next_public" "$scratch/actual-next.manifest"
    /usr/bin/cmp -- "$scratch/preexisting-next.manifest" \
        "$scratch/actual-next.manifest"
    assert_absent "$previous_public"
    assert_prior_public
    /usr/bin/rm -rf -- "$next_public"
fi

if should_run race-previous; then
    staged_public="$scratch/staged-race-previous/public"
    next_public="$copy/.public-next.race-previous"
    previous_public="$copy/.public-previous.race"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv "$move_into_raced_destination" /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: raced previous path unexpectedly accepted publication' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    test -f "$previous_public/racer"
    test ! -e "$previous_public/prior"
    assert_prior_public
    /usr/bin/rm -rf -- "$previous_public"
fi

if should_run empty-next; then
    staged_public="$scratch/staged-empty-next/public"
    next_public="$copy/.public-next.empty-next"
    previous_public="$copy/.public-previous.empty-next"
    foreign_identity="$scratch/empty-next.identity"
    make_staged_public "$staged_public"
    snapshot_tree "$staged_public" "$scratch/expected-empty-next-source.manifest"
    if FOREIGN_IDENTITY_RECORD="$foreign_identity" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$move_into_empty_destination" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: foreign empty next destination was clobbered' >&2
        exit 1
    fi
    snapshot_tree "$staged_public" "$scratch/actual-empty-next-source.manifest"
    /usr/bin/cmp -- "$scratch/expected-empty-next-source.manifest" \
        "$scratch/actual-empty-next-source.manifest"
    assert_recorded_identity "$next_public/tree" "$foreign_identity"
    assert_absent "$previous_public"
    assert_prior_public
    /usr/bin/rm -rf -- "$next_public"
fi

if should_run empty-previous; then
    staged_public="$scratch/staged-empty-previous/public"
    next_public="$copy/.public-next.empty-previous"
    previous_public="$copy/.public-previous.empty-previous"
    foreign_identity="$scratch/empty-previous.identity"
    make_staged_public "$staged_public"
    if FOREIGN_IDENTITY_RECORD="$foreign_identity" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv "$move_into_empty_destination" /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: foreign empty previous destination was clobbered' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_recorded_identity "$previous_public" "$foreign_identity"
    assert_prior_public
    /usr/bin/rmdir -- "$previous_public"
fi

if should_run empty-public; then
    staged_public="$scratch/staged-empty-public/public"
    next_public="$copy/.public-next.empty-public"
    previous_public="$copy/.public-previous.empty-public"
    foreign_identity="$scratch/empty-public.identity"
    make_staged_public "$staged_public"
    if FOREIGN_IDENTITY_RECORD="$foreign_identity" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$move_into_empty_destination" /usr/bin/mv; then
        printf '%s\n' 'error: foreign empty public destination was clobbered' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_recorded_identity "$public" "$foreign_identity"
    snapshot_tree "$previous_public" "$scratch/empty-public-prior.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/empty-public-prior.manifest"
    /usr/bin/rmdir -- "$public"
    /usr/bin/mv -T -- "$previous_public" "$public"
    assert_prior_public
fi

if should_run empty-restore; then
    staged_public="$scratch/staged-empty-restore/public"
    next_public="$copy/.public-next.empty-restore"
    previous_public="$copy/.public-previous.empty-restore"
    foreign_identity="$scratch/empty-restore.identity"
    make_staged_public "$staged_public"
    if FOREIGN_IDENTITY_RECORD="$foreign_identity" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/false "$move_into_empty_destination"; then
        printf '%s\n' 'error: foreign empty restore destination was clobbered' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_recorded_identity "$public" "$foreign_identity"
    snapshot_tree "$previous_public" "$scratch/empty-restore-prior.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/empty-restore-prior.manifest"
    /usr/bin/rmdir -- "$public"
    /usr/bin/mv -T -- "$previous_public" "$public"
    assert_prior_public
fi

if should_run quarantine-collision; then
    staged_public="$scratch/staged-quarantine-collision/public"
    next_public="$copy/.public-next.quarantine-collision"
    previous_public="$copy/.public-previous.quarantine-collision"
    foreign_identity="$scratch/quarantine-collision.identity"
    make_staged_public "$staged_public"
    if LIVE_ROOT_FOR_COLLISION="$copy" FOREIGN_IDENTITY_RECORD="$foreign_identity" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$move_then_add_quarantine_collision" /usr/bin/mv; then
        printf '%s\n' 'error: quarantine collision unexpectedly reported success' >&2
        exit 1
    fi
    quarantine=$(/usr/bin/find "$copy" -maxdepth 1 -type d \
        -name '.public-quarantine.*' -print -quit)
    test -n "$quarantine"
    assert_recorded_identity "$quarantine/finalize-next.0" "$foreign_identity"
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
    /usr/bin/rm -rf -- "$quarantine"
fi

if should_run identity-race; then
    staged_public="$scratch/staged-identity-race/public"
    next_public="$copy/.public-next.identity-race"
    previous_public="$copy/.public-previous.identity-race"
    next_tree="$next_public/tree"
    hidden_next_tree="$scratch/hidden-next-tree"
    final_move_was_called="$scratch/final-move-was-called"
    make_staged_public "$staged_public"
    if NEXT_TREE_TO_HIDE="$next_tree" HIDDEN_NEXT_TREE="$hidden_next_tree" \
            FINAL_MOVE_WAS_CALLED="$final_move_was_called" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv "$hide_next_during_old_move" \
            "$restore_hidden_then_move" /usr/bin/mv; then
        printf '%s\n' 'error: unstable next-tree identity unexpectedly accepted publication' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_absent "$final_move_was_called"
    test -d "$hidden_next_tree" && test ! -L "$hidden_next_tree"
    assert_prior_public
    /usr/bin/rm -rf -- "$hidden_next_tree"
fi

if should_run foreign-next; then
    staged_public="$scratch/staged-foreign-next/public"
    next_public="$copy/.public-next.foreign-next"
    previous_public="$copy/.public-previous.foreign-next"
    owned_next_hidden="$scratch/owned-next-hidden"
    foreign_next_source="$scratch/foreign-next-source"
    make_staged_public "$staged_public"
    snapshot_tree "$staged_public" "$scratch/expected-hidden-next.manifest"
    make_foreign_tree "$foreign_next_source" foreign-next
    snapshot_tree "$foreign_next_source" "$scratch/expected-foreign-next.manifest"
    if NEXT_WRAPPER_TO_SUBSTITUTE="$next_public" \
            OWNED_NEXT_HIDDEN="$owned_next_hidden" \
            FOREIGN_NEXT_SOURCE="$foreign_next_source" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$substitute_next_then_fail" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: substituted next wrapper unexpectedly accepted publication' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    snapshot_tree "$next_public" "$scratch/actual-foreign-next.manifest"
    /usr/bin/cmp -- "$scratch/expected-foreign-next.manifest" \
        "$scratch/actual-foreign-next.manifest"
    snapshot_tree "$owned_next_hidden/tree" "$scratch/actual-hidden-next.manifest"
    /usr/bin/cmp -- "$scratch/expected-hidden-next.manifest" \
        "$scratch/actual-hidden-next.manifest"
    assert_absent "$previous_public"
    assert_prior_public
    /usr/bin/rm -rf -- "$next_public" "$owned_next_hidden"
fi

if should_run foreign-public; then
    staged_public="$scratch/staged-foreign-public/public"
    next_public="$copy/.public-next.foreign-public"
    previous_public="$copy/.public-previous.foreign-public"
    owned_public_hidden="$scratch/owned-public-hidden"
    foreign_public_source="$scratch/foreign-public-source"
    make_staged_public "$staged_public"
    snapshot_tree "$staged_public" "$scratch/expected-new-public.manifest"
    make_foreign_tree "$foreign_public_source" foreign-public
    snapshot_tree "$foreign_public_source" "$scratch/expected-foreign-public.manifest"
    if PUBLIC_TO_SUBSTITUTE="$public" OWNED_PUBLIC_HIDDEN="$owned_public_hidden" \
            FOREIGN_PUBLIC_SOURCE="$foreign_public_source" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/mv /usr/bin/mv \
            "$substitute_public_verifier" "$final_tree_ledger" \
            aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; then
        printf '%s\n' 'error: substituted public tree unexpectedly accepted publication' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    snapshot_tree "$public" "$scratch/actual-foreign-public.manifest"
    /usr/bin/cmp -- "$scratch/expected-foreign-public.manifest" \
        "$scratch/actual-foreign-public.manifest"
    snapshot_tree "$owned_public_hidden" "$scratch/actual-hidden-new.manifest"
    /usr/bin/cmp -- "$scratch/expected-new-public.manifest" \
        "$scratch/actual-hidden-new.manifest"
    snapshot_tree "$previous_public" "$scratch/actual-recoverable-prior.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/actual-recoverable-prior.manifest"
    /usr/bin/rm -rf -- "$public" "$owned_public_hidden"
    /usr/bin/mv -T -- "$previous_public" "$public"
    assert_prior_public
fi

if should_run foreign-previous; then
    staged_public="$scratch/staged-foreign-previous/public"
    next_public="$copy/.public-next.foreign-previous"
    previous_public="$copy/.public-previous.foreign-previous"
    owned_previous_hidden="$scratch/owned-previous-hidden"
    foreign_previous_source="$scratch/foreign-previous-source"
    make_staged_public "$staged_public"
    make_foreign_tree "$foreign_previous_source" foreign-previous
    snapshot_tree "$foreign_previous_source" \
        "$scratch/expected-foreign-previous.manifest"
    if PREVIOUS_TO_SUBSTITUTE="$previous_public" \
            OWNED_PREVIOUS_HIDDEN="$owned_previous_hidden" \
            FOREIGN_PREVIOUS_SOURCE="$foreign_previous_source" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$substitute_previous_after_move" /usr/bin/mv; then
        printf '%s\n' 'error: substituted recovery tree incorrectly reported success' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$public"
    snapshot_tree "$previous_public" "$scratch/actual-foreign-previous.manifest"
    /usr/bin/cmp -- "$scratch/expected-foreign-previous.manifest" \
        "$scratch/actual-foreign-previous.manifest"
    snapshot_tree "$owned_previous_hidden" "$scratch/actual-hidden-prior.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/actual-hidden-prior.manifest"
    /usr/bin/rm -rf -- "$previous_public"
    /usr/bin/mv -T -- "$owned_previous_hidden" "$public"
    assert_prior_public
fi

if should_run cleanup-signal; then
    staged_public="$scratch/staged-cleanup-signal/public"
    next_public="$copy/.public-next.cleanup-signal"
    previous_public="$copy/.public-previous.cleanup-signal"
    make_staged_public "$staged_public"
    if NEXT_WRAPPER_TO_WATCH="$next_public" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$signal_during_cleanup_removal" /usr/bin/mv; then
        printf '%s\n' 'error: cleanup-removal signal unexpectedly reported success' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run restore-signal; then
    staged_public="$scratch/staged-restore-signal/public"
    next_public="$copy/.public-next.restore-signal"
    previous_public="$copy/.public-previous.restore-signal"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/false "$signal_during_restore"; then
        printf '%s\n' 'error: restore-boundary signal unexpectedly reported success' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run rollback; then
    staged_public="$scratch/staged-rollback/public"
    next_public="$copy/.public-next.rollback"
    previous_public="$copy/.public-previous.rollback"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/false /usr/bin/mv; then
        printf '%s\n' 'error: failing publication move unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run final-move-fail; then
    staged_public="$scratch/staged-final-move-fail/public"
    next_public="$copy/.public-next.final-move-fail"
    previous_public="$copy/.public-previous.final-move-fail"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$move_then_fail" /usr/bin/mv; then
        printf '%s\n' 'error: final rename followed by failure unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run final-move-signal; then
    staged_public="$scratch/staged-final-move-signal/public"
    next_public="$copy/.public-next.final-move-signal"
    previous_public="$copy/.public-previous.final-move-signal"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$move_then_signal" /usr/bin/mv; then
        printf '%s\n' 'error: final rename followed by signal unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run verified-race; then
    staged_public="$scratch/staged-verified-race/public"
    next_public="$copy/.public-next.verified-race"
    previous_public="$copy/.public-previous.verified-race"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv "$move_then_mutate" /usr/bin/mv \
            "$final_tree_verifier" "$final_tree_ledger" \
            aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; then
        printf '%s\n' 'error: mutation after final verification was published' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run publish-noop; then
    staged_public="$scratch/staged-publish-noop/public"
    next_public="$copy/.public-next.publish-noop"
    previous_public="$copy/.public-previous.publish-noop"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/false /usr/bin/mv; then
        printf '%s\n' 'error: publication without a completed move unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run restore-failure; then
    staged_public="$scratch/staged-restore-failure/public"
    next_public="$copy/.public-next.restore-failure"
    previous_public="$copy/.public-previous.restore-failure"
    make_staged_public "$staged_public"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/false /usr/bin/false; then
        printf '%s\n' 'error: failed publication and restoration unexpectedly succeeded' >&2
        exit 1
    fi
    assert_absent "$staged_public"
    assert_absent "$public"
    assert_absent "$next_public"
    test -d "$previous_public" && test ! -L "$previous_public"
    snapshot_tree "$previous_public" "$scratch/previous-public.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/previous-public.manifest"
    /usr/bin/mv -T -- "$previous_public" "$public"
    assert_absent "$previous_public"
    assert_prior_public
fi

if should_run retirement-failure; then
    staged_public="$scratch/staged-retirement-failure/public"
    next_public="$copy/.public-next.retirement-failure"
    previous_public="$copy/.public-previous.retirement-failure"
    make_staged_public "$staged_public"
    snapshot_tree "$staged_public" "$scratch/expected-retirement-live.manifest"
    set +e
    retirement_error=$(REAL_PUBLISH_RENAME_TOOL="$trusted_tool" \
        PUBLISH_RENAME_TOOL="$retirement_failure_tool" \
        sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/mv /usr/bin/mv 2>&1)
    retirement_result=$?
    set -e
    if test "$retirement_result" -eq 0; then
        printf '%s\n' 'error: failed prior retirement unexpectedly succeeded' >&2
        exit 1
    fi
    case "$retirement_error" in
        *'error: publication cleanup failed'*) ;;
        *)
            printf '%s\n' 'error: failed prior retirement was not visible' >&2
            exit 1
            ;;
    esac
    assert_absent "$staged_public"
    assert_absent "$next_public"
    assert_absent "$previous_public"
    if test ! -d "$public" || test -L "$public"; then
        printf '%s\n' 'error: failed prior retirement removed the verified live tree' >&2
        exit 1
    fi
    snapshot_tree "$public" "$scratch/actual-retirement-live.manifest"
    /usr/bin/cmp -- "$scratch/expected-retirement-live.manifest" \
        "$scratch/actual-retirement-live.manifest"
    quarantine=$(/usr/bin/find "$copy" -maxdepth 1 -type d \
        -name '.public-quarantine.*' -print -quit)
    test -n "$quarantine"
    snapshot_tree "$quarantine/retired-prior" \
        "$scratch/actual-retired-prior.manifest"
    /usr/bin/cmp -- "$scratch/prior-public.manifest" \
        "$scratch/actual-retired-prior.manifest"
    test "$(/usr/bin/cat "$quarantine/finalize-previous.0/foreign.txt")" = foreign
    /usr/bin/rm -rf -- "$quarantine"
fi

if /usr/bin/find "$copy" -maxdepth 1 \
        \( -name '.public-next.*' -o -name '.public-previous.*' \
            -o -name '.public-quarantine.*' \) \
        -print -quit | /usr/bin/grep .; then
    printf '%s\n' 'error: publication transient remains' >&2
    exit 1
fi
snapshot_tracked "$tracked_after"
/usr/bin/cmp -s -- "$tracked_before" "$tracked_after"
