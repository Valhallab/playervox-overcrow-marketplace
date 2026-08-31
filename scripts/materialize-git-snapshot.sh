#!/bin/sh
set -eu
umask 077

mode=${1:-}
case "$mode:$#" in
    --bootstrap:4 | --validated:5) ;;
    *)
        printf '%s\n' \
            'usage: materialize-git-snapshot.sh --bootstrap REPOSITORY REVISION OUTPUT | --validated REPOSITORY REVISION OUTPUT TRUSTED-TOOL' >&2
        exit 2
        ;;
esac
repository=$2
revision=$3
output=$4
trusted_tool=${5:-}
case "$repository:$output" in
    /*:/*) ;;
    *) printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2; exit 1 ;;
esac
case "$revision" in
    *[!0-9a-f]* | '')
        printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
        exit 1
        ;;
esac
if test "${#revision}" -ne 40; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi

invoking_uid=$(/usr/bin/id -u)
output_parent=$(/usr/bin/dirname -- "$output")
if test "$repository" = / || test "$output" = / \
        || test ! -d "$repository" || test -L "$repository" \
        || test "$(CDPATH='' cd -- "$repository" && pwd -P)" != "$repository" \
        || test "$(/usr/bin/stat -c '%u' "$repository")" != "$invoking_uid" \
        || /usr/bin/find "$repository" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null \
        || test -e "$output" || test -L "$output" \
        || test ! -d "$output_parent" || test -L "$output_parent" \
        || test "$(CDPATH='' cd -- "$output_parent" && pwd -P)" != "$output_parent" \
        || test "$(/usr/bin/stat -c '%u' "$output_parent")" != "$invoking_uid" \
        || /usr/bin/find "$output_parent" -maxdepth 0 -perm /0022 -print -quit \
            | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi
for program in \
        /usr/bin/env /usr/bin/git /usr/bin/timeout /usr/bin/prlimit \
        /usr/bin/tar /usr/bin/find /usr/bin/stat /usr/bin/sha256sum; do
    if test ! -f "$program" || test -L "$program" \
            || test "$(/usr/bin/stat -c '%u:%a' "$program")" != 0:755; then
        printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
        exit 1
    fi
done

trusted_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/timeout --signal=KILL 15 \
        /usr/bin/prlimit --cpu=10 --as=1073741824 --nofile=128 \
            --fsize=33554432 -- \
        /usr/bin/git --no-replace-objects \
            -c core.fsmonitor=false -c core.hooksPath=/dev/null \
            -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
            -c diff.external= -C "$repository" "$@"
}

resolved_revision=$(trusted_git rev-parse --verify "$revision^{commit}" 2>/dev/null) \
    || resolved_revision=''
if test "$resolved_revision" != "$revision"; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi

work=$(/usr/bin/mktemp -d "${output}.build.XXXXXXXXXX") || exit 1
archive=$(/usr/bin/mktemp "${output_parent}/snapshot.XXXXXXXXXX.tar") || exit 1
plan=$(/usr/bin/mktemp "${output_parent}/snapshot-plan.XXXXXXXXXX") || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -f -- "$archive" "$plan"
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM
/usr/bin/chmod 0600 -- "$archive" "$plan"

if test "$mode" = --validated; then
    case "$trusted_tool" in
        /*) ;;
        *) printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2; exit 1 ;;
    esac
    if test ! -f "$trusted_tool" || test -L "$trusted_tool" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$trusted_tool")" \
                != "$invoking_uid:700:1" \
            || ! "$trusted_tool" snapshot-plan \
                --repository "$repository" --revision "$revision" >"$plan"; then
        printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
        exit 1
    fi
fi

if ! trusted_git archive --format=tar --output="$archive" "$revision" \
        || test ! -f "$archive" || test -L "$archive" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$archive")" \
            != "$invoking_uid:600:1" \
        || test "$(/usr/bin/stat -c '%s' "$archive")" -gt 33554432 \
        || ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
            /usr/bin/timeout --signal=KILL 15 \
            /usr/bin/prlimit --cpu=10 --as=1073741824 --nofile=128 \
                --fsize=33554432 -- \
            /usr/bin/tar --extract --file="$archive" --directory="$work" \
                --no-same-owner --no-same-permissions --delay-directory-restore; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi

file_count=$(/usr/bin/find "$work" -xdev -type f -printf . | /usr/bin/wc -c)
byte_count=$(/usr/bin/find "$work" -xdev -type f -printf '%s\n' \
    | /usr/bin/awk '{ total += $1 } END { print total + 0 }')
if test "$file_count" -eq 0 || test "$file_count" -gt 1000 \
        || test "$byte_count" -gt 16777216 \
        || test -n "$(/usr/bin/find "$work" -xdev ! -type d ! -type f -print -quit)" \
        || test -n "$(/usr/bin/find "$work" -xdev ! -user "$invoking_uid" -print -quit)" \
        || test -n "$(/usr/bin/find "$work" -xdev -type f -size +8388608c -print -quit)" \
        || test -n "$(/usr/bin/find "$work" -xdev -perm /0022 -print -quit)"; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi

if test "$mode" = --validated; then
    tab=$(printf '\t')
    checked_entries=0
    checked_bytes=0
    while IFS="$tab" read -r expected_mode expected_size expected_oid relative; do
        case "$expected_mode:$expected_size:$expected_oid:$relative" in
            100644:* | 100755:*) ;;
            *) printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2; exit 1 ;;
        esac
        file="$work/$relative"
        if test ! -f "$file" || test -L "$file" \
                || test "$(/usr/bin/stat -c '%s' "$file")" != "$expected_size" \
                || test "$(trusted_git hash-object --no-filters -- "$file" 2>/dev/null)" \
                    != "$expected_oid"; then
            printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
            exit 1
        fi
        if test "$expected_mode" = 100644; then
            test -z "$(/usr/bin/find "$file" -maxdepth 0 -perm /0111 -print -quit)" \
                || { printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2; exit 1; }
        else
            test -n "$(/usr/bin/find "$file" -maxdepth 0 -perm /0100 -print -quit)" \
                || { printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2; exit 1; }
        fi
        checked_entries=$((checked_entries + 1))
        checked_bytes=$((checked_bytes + expected_size))
        if test "$checked_entries" -gt 1000 || test "$checked_bytes" -gt 16777216; then
            printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
            exit 1
        fi
    done <"$plan"
    if test "$checked_entries" -ne "$file_count" \
            || test "$checked_bytes" -ne "$byte_count"; then
        printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
        exit 1
    fi
fi

/usr/bin/rm -f -- "$archive" "$plan"
if test -e "$output" || test -L "$output"; then
    printf '%s\n' 'error: trusted Git snapshot is unavailable' >&2
    exit 1
fi
/usr/bin/mv -T -- "$work" "$output"
work=''
trap - EXIT HUP INT TERM
