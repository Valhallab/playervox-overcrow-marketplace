#!/bin/sh
set -eu

if test "$#" -gt 1; then
    printf '%s\n' \
        'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|rollback|verified-race|publish-noop|restore-failure]' >&2
    exit 2
fi
selected_case=${1:-all}
case "$selected_case" in
    all | validation | signal | post-move | race-next | race-previous | rollback | verified-race | publish-noop | restore-failure) ;;
    *)
        printf '%s\n' \
            'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|rollback|verified-race|publish-noop|restore-failure]' >&2
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
helper="$repo_root/scripts/publish-directory.sh"
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
    '/usr/bin/mv "$@"' \
    '/usr/bin/kill -TERM "$PPID"' >"$move_then_signal"
/usr/bin/chmod 0700 "$move_then_signal"

move_then_fail="$scratch/move-then-fail"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    '/usr/bin/mv "$@"' \
    '/usr/bin/false' >"$move_then_fail"
/usr/bin/chmod 0700 "$move_then_fail"

move_into_raced_destination="$scratch/move-into-raced-destination"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'destination=$4' \
    '/usr/bin/mkdir -p "$destination"' \
    'printf "%s\n" racer >"$destination/racer"' \
    '/usr/bin/mv "$@"' >"$move_into_raced_destination"
/usr/bin/chmod 0700 "$move_into_raced_destination"

move_then_mutate="$scratch/move-then-mutate"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    '/usr/bin/mv "$@"' \
    'printf "%s\n" raced >"$4/raced-after-verification"' >"$move_then_mutate"
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
            /usr/bin/mv /usr/bin/mv /usr/bin/true /usr/bin/mv; then
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

if /usr/bin/find "$copy" -maxdepth 1 \
        \( -name '.public-next.*' -o -name '.public-previous.*' \) \
        -print -quit | /usr/bin/grep .; then
    printf '%s\n' 'error: publication transient remains' >&2
    exit 1
fi
snapshot_tracked "$tracked_after"
/usr/bin/cmp -s -- "$tracked_before" "$tracked_after"
