#!/bin/sh
set -eu

if test "$#" -ne 1; then
    printf '%s\n' 'usage: sync-wit.sh /absolute/path/to/overcrow-worktree' >&2
    exit 2
fi

case "$1" in
    /*) ;;
    *)
        printf '%s\n' 'error: OverCrow worktree path must be absolute' >&2
        exit 1
        ;;
esac

worktree=$1
canonical_worktree=$(/usr/bin/readlink -f -- "$worktree") || {
    printf '%s\n' 'error: OverCrow worktree is unavailable' >&2
    exit 1
}
if test "$worktree" != "$canonical_worktree" || test ! -d "$worktree" || test -L "$worktree"; then
    printf '%s\n' 'error: OverCrow worktree must be a canonical non-symlink directory' >&2
    exit 1
fi

logical_script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -L)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(dirname -- "$script_dir")
canonical_repo=$(/usr/bin/readlink -f -- "$repo_root") || {
    printf '%s\n' 'error: marketplace repository is unavailable' >&2
    exit 1
}
script_file="$script_dir/$(basename -- "$0")"
if test "$logical_script_dir" != "$script_dir" || test "$canonical_repo" != "$repo_root" \
        || test ! -d "$repo_root" || test -L "$repo_root" \
        || test ! -f "$script_file" || test -L "$script_file"; then
    printf '%s\n' 'error: marketplace repository must be canonical and non-symlinked' >&2
    exit 1
fi

destination="$repo_root/wit"
previous="$repo_root/.wit-previous"
stage=''
backup_created=false

exec 9<"$repo_root"
if ! /usr/bin/flock -n 9; then
    printf '%s\n' 'error: WIT synchronization is already in progress' >&2
    exit 1
fi

valid_pair() {
    test -d "$1" && test ! -L "$1" \
        && test -f "$1/widget-v1.wit" && test ! -L "$1/widget-v1.wit" \
        && test -f "$1/widget-v1.sha256" && test ! -L "$1/widget-v1.sha256" \
        && test -f "$1/widget-v2.wit" && test ! -L "$1/widget-v2.wit" \
        && test -f "$1/widget-v2.sha256" && test ! -L "$1/widget-v2.sha256" \
        || return 1
    pair_extra=$(/usr/bin/find "$1" -mindepth 1 -maxdepth 1 \
        ! -name widget-v1.wit ! -name widget-v1.sha256 \
        ! -name widget-v2.wit ! -name widget-v2.sha256 -print -quit) || return 1
    test -z "$pair_extra" || return 1
    for pair_version in v1 v2; do
        pair_hash=$(/usr/bin/sha256sum "$1/widget-$pair_version.wit") || return 1
        pair_hash=${pair_hash%% *}
        pair_recorded=$(/usr/bin/cat "$1/widget-$pair_version.sha256") || return 1
        case "$pair_recorded" in
            *[!0-9a-f]* | '') return 1 ;;
        esac
        test "${#pair_recorded}" -eq 64 \
            && test "$pair_hash" = "$pair_recorded" || return 1
    done
}

restore_previous() {
    exit_code=$?
    trap - EXIT HUP INT TERM
    if test -n "$stage" && { test -e "$stage" || test -L "$stage"; }; then
        if test -L "$stage" || test ! -d "$stage"; then
            /usr/bin/rm -f -- "$stage" || exit_code=1
        else
            /usr/bin/rm -rf -- "$stage" || exit_code=1
        fi
    fi
    if test "$backup_created" = true && valid_pair "$previous" \
            && test ! -e "$destination" && test ! -L "$destination"; then
        /usr/bin/mv -T -- "$previous" "$destination" || exit_code=1
    fi
    exit "$exit_code"
}
trap restore_previous EXIT
trap 'exit 1' HUP INT TERM

if test -e "$previous" || test -L "$previous"; then
    if ! valid_pair "$previous"; then
        printf '%s\n' 'error: interrupted WIT backup is unsafe or incomplete' >&2
        exit 1
    fi
    if test -e "$destination" || test -L "$destination"; then
        printf '%s\n' 'error: WIT output and interrupted backup both exist; preserving both' >&2
        exit 1
    fi
    /usr/bin/mv -T -- "$previous" "$destination"
fi

if { test -e "$destination" || test -L "$destination"; } && ! valid_pair "$destination"; then
    printf '%s\n' 'error: existing WIT output is unsafe, incomplete, or drifted' >&2
    exit 1
fi

source_wit_v1="$worktree/crates/overcrow-extension-api/wit/widget-v1.wit"
source_wit_v2="$worktree/crates/overcrow-extension-api/wit/widget-v2.wit"
source_spec="$worktree/docs/superpowers/specs/2026-08-25-widget-marketplace-design.md"
for source_file in "$source_wit_v1" "$source_wit_v2" "$source_spec"; do
    canonical_source=$(/usr/bin/readlink -f -- "$source_file") || {
        printf '%s\n' 'error: required OverCrow source is unavailable' >&2
        exit 1
    }
    if test "$canonical_source" != "$source_file" || test ! -f "$source_file" || test -L "$source_file"; then
        printf '%s\n' 'error: required OverCrow source must be canonical and regular' >&2
        exit 1
    fi
done

stage=$(/usr/bin/mktemp -d "$repo_root/.wit-sync.XXXXXXXXXX") || {
    printf '%s\n' 'error: could not create exclusive WIT staging directory' >&2
    exit 1
}
canonical_stage=$(/usr/bin/readlink -f -- "$stage") || exit 1
if test "$canonical_stage" != "$stage" || test ! -d "$stage" || test -L "$stage"; then
    printf '%s\n' 'error: WIT staging directory is unsafe' >&2
    exit 1
fi
/usr/bin/chmod 0700 "$stage"
for source_version in v1 v2; do
    case "$source_version" in
        v1) source_file=$source_wit_v1 ;;
        v2) source_file=$source_wit_v2 ;;
    esac
    /usr/bin/install -m 0644 "$source_file" "$stage/widget-$source_version.wit"
    source_hash=$(/usr/bin/sha256sum "$stage/widget-$source_version.wit")
    source_hash=${source_hash%% *}
    case "$source_hash" in
        *[!0-9a-f]* | '')
            printf '%s\n' 'error: SHA-256 output is invalid' >&2
            exit 1
            ;;
    esac
    if test "${#source_hash}" -ne 64; then
        printf '%s\n' 'error: SHA-256 output is invalid' >&2
        exit 1
    fi
    printf '%s\n' "$source_hash" >"$stage/widget-$source_version.sha256"
done

if test -e "$destination"; then
    backup_created=true
    /usr/bin/mv -T -- "$destination" "$previous"
fi
/usr/bin/mv -T -- "$stage" "$destination"
stage=''
valid_pair "$destination" || {
    printf '%s\n' 'error: synchronized WIT failed post-publication validation' >&2
    exit 1
}
if test -e "$previous"; then
    /usr/bin/rm -rf -- "$previous"
fi
backup_created=false
printf '%s\n' "$(/usr/bin/cat "$destination/widget-v1.sha256")"
printf '%s\n' "$(/usr/bin/cat "$destination/widget-v2.sha256")"
