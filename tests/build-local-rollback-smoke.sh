#!/bin/sh
set -eu

if test "$#" -gt 1; then
    printf '%s\n' \
        'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|rollback|publish-noop|restore-failure]' >&2
    exit 2
fi
selected_case=${1:-all}
case "$selected_case" in
    all | validation | signal | post-move | race-next | race-previous | rollback | publish-noop | restore-failure) ;;
    *)
        printf '%s\n' \
            'usage: build-local-rollback-smoke.sh [validation|signal|post-move|race-next|race-previous|rollback|publish-noop|restore-failure]' >&2
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

assert_prior_public() {
    /usr/bin/diff --recursive --no-dereference "$scratch/prior-public" "$public"
    test ! -L "$public"
    test "$(CDPATH='' cd -- "$public" && pwd -P)" = "$public"
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

race_next_reservation="$scratch/race-next-reservation"
# shellcheck disable=SC2016 # generated helper expands these variables when run
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'if /usr/bin/mkdir -m 0700 -- "$RACED_NEXT_PUBLIC" 2>/dev/null; then' \
    '    /usr/bin/mkdir -- "$RACED_NEXT_PUBLIC/foreign"' \
    '    printf "%s\n" foreign-owned >"$RACED_NEXT_PUBLIC/foreign/owned.txt"' \
    '    : >"$RACE_WON"' \
    'fi' \
    '/usr/bin/false' >"$race_next_reservation"
/usr/bin/chmod 0700 "$race_next_reservation"

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
    race_won="$scratch/race-next-won"
    make_staged_public "$staged_public"
    if RACED_NEXT_PUBLIC="$next_public" RACE_WON="$race_won" \
            sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            "$race_next_reservation" /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: raced next path unexpectedly accepted publication' >&2
        exit 1
    fi
    test -f "$staged_public/next"
    foreign_marker="$next_public/foreign/owned.txt"
    if test -f "$race_won"; then
        test -f "$foreign_marker"
        test "$(/usr/bin/cat "$foreign_marker")" = foreign-owned
        /usr/bin/rm -rf -- "$next_public"
    else
        assert_absent "$next_public"
    fi
    assert_absent "$previous_public"
    assert_prior_public

    /usr/bin/mkdir -m 0700 -- "$next_public"
    /usr/bin/mkdir -- "$next_public/foreign"
    printf '%s\n' foreign-owned >"$foreign_marker"
    if sh "$helper" "$staged_public" "$public" "$next_public" "$previous_public" \
            /usr/bin/mv /usr/bin/mv /usr/bin/mv /usr/bin/mv; then
        printf '%s\n' 'error: foreign next path unexpectedly accepted publication' >&2
        exit 1
    fi
    test -f "$staged_public/next"
    test -f "$foreign_marker"
    test "$(/usr/bin/cat "$foreign_marker")" = foreign-owned
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
    /usr/bin/diff --recursive --no-dereference "$scratch/prior-public" "$previous_public"
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
