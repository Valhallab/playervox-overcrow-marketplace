#!/bin/sh
set -eu
umask 077

usage() {
    printf '%s\n' \
        'usage: review-bundle.sh create --source ABSOLUTE --output ABSOLUTE --trust-sha SHA --review-sha SHA --review-tree TREE | review-bundle.sh verify --bundle ABSOLUTE --review-sha SHA --review-tree TREE [--trust-sha SHA] | review-bundle.sh verify-copy --bundle ABSOLUTE --copy ABSOLUTE --review-sha SHA --review-tree TREE | review-bundle.sh rebind --bundle ABSOLUTE --from-review-sha SHA --to-review-sha SHA --review-tree TREE' >&2
    exit 2
}

valid_object_id() {
    case "$1" in '' | *[!0-9a-f]*) return 1 ;; esac
    test "${#1}" -eq 40
}

safe_directory() {
    directory=$1
    mode=$2
    test -d "$directory" && test ! -L "$directory" \
        && test "$(CDPATH='' cd -- "$directory" 2>/dev/null && pwd -P || :)" \
            = "$directory" \
        && test "$(/usr/bin/stat -c '%u:%a' "$directory" 2>/dev/null || :)" \
            = "$(/usr/bin/id -u):$mode"
}

safe_owned_directory() {
    directory=$1
    test -d "$directory" && test ! -L "$directory" \
        && test "$(CDPATH='' cd -- "$directory" 2>/dev/null && pwd -P || :)" \
            = "$directory" \
        && test "$(/usr/bin/stat -c '%u' "$directory" 2>/dev/null || :)" \
            = "$(/usr/bin/id -u)" \
        && test -z "$(/usr/bin/find "$directory" -maxdepth 0 \
            -perm /0022 -print -quit)"
}

write_ledger() {
    ledger_root=$1
    ledger_output=$2
    ledger_entries=${ledger_output}.entries
    ledger_owner=$(/usr/bin/id -u)
    safe_owned_directory "$ledger_root" \
        || return 1
    /usr/bin/find "$ledger_root" -xdev -mindepth 1 -printf '%P\n' \
        | LC_ALL=C /usr/bin/sort >"$ledger_entries" || return 1
    : >"$ledger_output"
    /usr/bin/chmod 0600 "$ledger_output" "$ledger_entries"
    ledger_count=0
    ledger_aggregate=0
    while IFS= read -r relative; do
        case "$relative" in
            '' | /* | */ | *[!A-Za-z0-9._+@/-]* | *//* | ../* | */../* | */.. | ./* | */./* | */.)
                return 1
                ;;
        esac
        test "${#relative}" -le 240 || return 1
        path="$ledger_root/$relative"
        properties=$(/usr/bin/stat -c '%F:%u:%a:%h:%s' "$path" 2>/dev/null) \
            || return 1
        kind=${properties%%:*}
        remainder=${properties#*:}
        file_owner=${remainder%%:*}
        remainder=${remainder#*:}
        mode=${remainder%%:*}
        remainder=${remainder#*:}
        links=${remainder%%:*}
        size=${remainder#*:}
        test "$file_owner" = "$ledger_owner" \
            && test -z "$(/usr/bin/find "$path" -maxdepth 0 -perm /0022 -print -quit)" \
            || return 1
        case "$kind:$links:$size" in
            directory:*:*)
                printf 'd\t%s\t%s\n' "$mode" "$relative" >>"$ledger_output"
                ;;
            'regular file':1:*)
                case "$size" in '' | *[!0-9]*) return 1 ;; esac
                test "$size" -le 8388608 || return 1
                ledger_aggregate=$((ledger_aggregate + size))
                test "$ledger_aggregate" -le 67108864 || return 1
                digest=$(/usr/bin/sha256sum "$path" | /usr/bin/cut -d ' ' -f 1) \
                    || return 1
                case "$digest" in '' | *[!0-9a-f]*) return 1 ;; esac
                test "${#digest}" -eq 64 || return 1
                printf 'f\t%s\t%s\t%s\t%s\n' \
                    "$mode" "$size" "$digest" "$relative" >>"$ledger_output"
                ;;
            *) return 1 ;;
        esac
        ledger_count=$((ledger_count + 1))
        test "$ledger_count" -le 2000 || return 1
    done <"$ledger_entries"
    /usr/bin/rm -f -- "$ledger_entries"
    test "$ledger_count" -gt 0 \
        && test "$(/usr/bin/stat -c '%u:%a:%h' "$ledger_output")" \
            = "$ledger_owner:600:1" \
        && test "$(/usr/bin/stat -c '%s' "$ledger_output")" -le 1048576
}

create_bundle() {
    test "$#" -eq 10 || usage
    test "$1" = --source && test "$3" = --output \
        && test "$5" = --trust-sha && test "$7" = --review-sha \
        && test "$9" = --review-tree || usage
    source_root=$2
    output=$4
    trust_sha=$6
    review_sha=$8
    review_tree=${10}
    if ! valid_object_id "$trust_sha" || ! valid_object_id "$review_sha" \
            || ! valid_object_id "$review_tree"; then
        printf '%s\n' 'error: review bundle identity is invalid' >&2
        exit 1
    fi
    case "$source_root:$output" in /*:/*) ;; *) usage ;; esac
    source_root=$(CDPATH='' cd -- "$source_root" 2>/dev/null && pwd -P) || {
        printf '%s\n' 'error: review bundle source is unsafe' >&2
        exit 1
    }
    output_parent=${output%/*}
    case "$output_parent" in '' | "$output") output_parent=/ ;; esac
    if ! safe_directory "$output_parent" 700 \
            || test -e "$output" || test -L "$output"; then
        printf '%s\n' 'error: review bundle destination is unsafe' >&2
        exit 1
    fi
    case "$output" in "$source_root" | "$source_root"/*)
        printf '%s\n' 'error: review bundle destination is unsafe' >&2
        exit 1
        ;;
    esac

    work=$(/usr/bin/mktemp -d "$output_parent/.review-bundle.XXXXXXXXXX") \
        || exit 1
    cleanup_create() {
        status=$?
        trap - EXIT HUP INT TERM
        /usr/bin/rm -rf -- "$work"
        exit "$status"
    }
    trap cleanup_create EXIT HUP INT TERM
    /usr/bin/chmod 0700 "$work"
    final="$work/final"
    repository="$final/repository"
    /usr/bin/install -d -m 0700 -- "$final" "$repository"

    before="$work/source-before.tsv"
    after="$work/source-after.tsv"
    if ! write_ledger "$source_root" "$before" \
            || ! /usr/bin/cp -a -- "$source_root/." "$repository/" \
            || ! write_ledger "$source_root" "$after" \
            || ! /usr/bin/cmp --silent "$before" "$after" \
            || ! write_ledger "$repository" "$final/ledger.tsv" \
            || ! /usr/bin/cmp --silent "$before" "$final/ledger.tsv"; then
        printf '%s\n' 'error: reviewed bytes changed while bundling' >&2
        exit 1
    fi
    ledger_digest=$(/usr/bin/sha256sum "$final/ledger.tsv" \
        | /usr/bin/cut -d ' ' -f 1)
    case "$ledger_digest" in '' | *[!0-9a-f]*) exit 1 ;; esac
    test "${#ledger_digest}" -eq 64
    printf '%s\n' \
        'schemaVersion=1' \
        "trustSha=$trust_sha" \
        "reviewRevision=$review_sha" \
        "reviewTree=$review_tree" \
        "ledgerSha256=$ledger_digest" >"$final/receipt"
    /usr/bin/chmod 0600 "$final/receipt" "$final/ledger.tsv"
    /usr/bin/sync -f "$final/receipt"
    /usr/bin/sync -f "$final/ledger.tsv"
    /usr/bin/sync -f "$repository"
    /usr/bin/mv -T -- "$final" "$output"
    /usr/bin/sync -f "$output_parent"
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
}

verify_bundle() {
    test "$#" -eq 6 || test "$#" -eq 8 || usage
    test "$1" = --bundle && test "$3" = --review-sha \
        && test "$5" = --review-tree || usage
    if test "$#" -eq 8; then
        test "$7" = --trust-sha || usage
        expected_trust=$8
    else
        expected_trust=''
    fi
    bundle=$2
    expected_review=$4
    expected_tree=$6
    if ! valid_object_id "$expected_review" || ! valid_object_id "$expected_tree" \
            || { test -n "$expected_trust" \
                && ! valid_object_id "$expected_trust"; }; then
        printf '%s\n' 'error: review bundle identity is invalid' >&2
        exit 1
    fi
    case "$bundle" in /*) ;; *) usage ;; esac
    if ! safe_directory "$bundle" 700 \
            || test "$(/usr/bin/find "$bundle" -mindepth 1 -maxdepth 1 \
                -printf . | /usr/bin/wc -c)" -ne 3 \
            || test ! -d "$bundle/repository" \
            || test ! -f "$bundle/ledger.tsv" || test -L "$bundle/ledger.tsv" \
            || test ! -f "$bundle/receipt" || test -L "$bundle/receipt" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$bundle/ledger.tsv")" \
                != "$(/usr/bin/id -u):600:1" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$bundle/receipt")" \
                != "$(/usr/bin/id -u):600:1"; then
        printf '%s\n' 'error: review bundle is unsafe' >&2
        exit 1
    fi
    test "$(/usr/bin/wc -l <"$bundle/receipt")" -eq 5 || {
        printf '%s\n' 'error: review bundle receipt is invalid' >&2
        exit 1
    }
    schema=$(/usr/bin/sed -n 's/^schemaVersion=//p' "$bundle/receipt")
    trust_sha=$(/usr/bin/sed -n 's/^trustSha=//p' "$bundle/receipt")
    review_sha=$(/usr/bin/sed -n 's/^reviewRevision=//p' "$bundle/receipt")
    review_tree=$(/usr/bin/sed -n 's/^reviewTree=//p' "$bundle/receipt")
    ledger_digest=$(/usr/bin/sed -n 's/^ledgerSha256=//p' "$bundle/receipt")
    case "$ledger_digest" in '' | *[!0-9a-f]*) ledger_digest='' ;; esac
    if test "$schema" != 1 || ! valid_object_id "$trust_sha" \
            || ! valid_object_id "$review_sha" \
            || ! valid_object_id "$review_tree" \
            || { test -n "$expected_trust" \
                && test "$trust_sha" != "$expected_trust"; } \
            || test "$review_sha" != "$expected_review" \
            || test "$review_tree" != "$expected_tree" \
            || test "${#ledger_digest}" -ne 64 \
            || test "$(/usr/bin/sha256sum "$bundle/ledger.tsv" \
                | /usr/bin/cut -d ' ' -f 1)" != "$ledger_digest"; then
        printf '%s\n' 'error: review bundle receipt is invalid' >&2
        exit 1
    fi

    verify_root=$(/usr/bin/mktemp -d /tmp/marketplace-bundle-verify.XXXXXXXXXX) \
        || exit 1
    cleanup_verify() {
        status=$?
        trap - EXIT HUP INT TERM
        /usr/bin/rm -rf -- "$verify_root"
        exit "$status"
    }
    trap cleanup_verify EXIT HUP INT TERM
    /usr/bin/chmod 0700 "$verify_root"
    first="$verify_root/first.tsv"
    second="$verify_root/second.tsv"
    if ! write_ledger "$bundle/repository" "$first" \
            || ! write_ledger "$bundle/repository" "$second" \
            || ! /usr/bin/cmp --silent "$first" "$second" \
            || ! /usr/bin/cmp --silent "$first" "$bundle/ledger.tsv"; then
        printf '%s\n' 'error: review bundle bytes are invalid' >&2
        exit 1
    fi
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$verify_root"
}

verify_copy() {
    test "$#" -eq 8 || usage
    test "$1" = --bundle && test "$3" = --copy \
        && test "$5" = --review-sha && test "$7" = --review-tree || usage
    bundle=$2
    copy_root=$4
    expected_review=$6
    expected_tree=$8
    if ! verify_bundle --bundle "$bundle" --review-sha "$expected_review" \
            --review-tree "$expected_tree"; then
        printf '%s\n' 'error: reviewed copy is invalid' >&2
        exit 1
    fi
    if ! safe_owned_directory "$copy_root"; then
        printf '%s\n' 'error: reviewed copy root is unsafe' >&2
        exit 1
    fi

    verify_root=$(/usr/bin/mktemp -d /tmp/marketplace-copy-verify.XXXXXXXXXX) \
        || exit 1
    cleanup_copy() {
        status=$?
        trap - EXIT HUP INT TERM
        /usr/bin/rm -rf -- "$verify_root"
        exit "$status"
    }
    trap cleanup_copy EXIT HUP INT TERM
    /usr/bin/chmod 0700 "$verify_root"
    first="$verify_root/first.tsv"
    second="$verify_root/second.tsv"
    if ! write_ledger "$copy_root" "$first" \
            || ! write_ledger "$copy_root" "$second"; then
        printf '%s\n' 'error: reviewed copy ledger failed' >&2
        exit 1
    fi
    if ! /usr/bin/cmp --silent "$first" "$second" \
            || ! /usr/bin/cmp --silent "$first" "$bundle/ledger.tsv"; then
        printf '%s\n' 'error: reviewed copy is invalid' >&2
        exit 1
    fi
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$verify_root"
}

rebind_bundle() {
    test "$#" -eq 8 || usage
    test "$1" = --bundle && test "$3" = --from-review-sha \
        && test "$5" = --to-review-sha && test "$7" = --review-tree || usage
    bundle=$2
    from_review=$4
    to_review=$6
    review_tree=$8
    if ! valid_object_id "$from_review" || ! valid_object_id "$to_review" \
            || ! valid_object_id "$review_tree" \
            || ! verify_bundle --bundle "$bundle" --review-sha "$from_review" \
                --review-tree "$review_tree"; then
        printf '%s\n' 'error: review bundle cannot be rebound' >&2
        exit 1
    fi
    trust_sha=$(/usr/bin/sed -n 's/^trustSha=//p' "$bundle/receipt")
    ledger_digest=$(/usr/bin/sed -n 's/^ledgerSha256=//p' "$bundle/receipt")
    bundle_parent=${bundle%/*}
    case "$bundle_parent" in '' | "$bundle") bundle_parent=/ ;; esac
    if ! safe_directory "$bundle_parent" 700; then
        printf '%s\n' 'error: review bundle cannot be rebound' >&2
        exit 1
    fi
    temporary=$(/usr/bin/mktemp "$bundle_parent/.review-receipt.XXXXXXXXXX") \
        || exit 1
    cleanup_rebind() {
        status=$?
        trap - EXIT HUP INT TERM
        /usr/bin/rm -f -- "$temporary"
        exit "$status"
    }
    trap cleanup_rebind EXIT HUP INT TERM
    if ! printf '%s\n' \
            'schemaVersion=1' \
            "trustSha=$trust_sha" \
            "reviewRevision=$to_review" \
            "reviewTree=$review_tree" \
            "ledgerSha256=$ledger_digest" >"$temporary" \
            || ! /usr/bin/chmod 0600 "$temporary" \
            || ! verify_bundle --bundle "$bundle" --review-sha "$from_review" \
                --review-tree "$review_tree" \
            || ! /usr/bin/sync -f "$temporary" \
            || ! /usr/bin/mv -T -- "$temporary" "$bundle/receipt" \
            || ! /usr/bin/sync -f "$bundle" \
            || ! verify_bundle --bundle "$bundle" --review-sha "$to_review" \
                --review-tree "$review_tree"; then
        printf '%s\n' 'error: review bundle cannot be rebound' >&2
        exit 1
    fi
    trap - EXIT HUP INT TERM
}

case "${1:-}" in
    create) shift; create_bundle "$@" ;;
    verify) shift; verify_bundle "$@" ;;
    verify-copy) shift; verify_copy "$@" ;;
    rebind) shift; rebind_bundle "$@" ;;
    *) usage ;;
esac
