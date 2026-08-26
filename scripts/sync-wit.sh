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

source_wit="$worktree/crates/overcrow-extension-api/wit/widget-v1.wit"
source_spec="$worktree/docs/superpowers/specs/2026-08-25-widget-marketplace-design.md"
for source_file in "$source_wit" "$source_spec"; do
    canonical_source=$(/usr/bin/readlink -f -- "$source_file") || {
        printf '%s\n' 'error: required OverCrow source is unavailable' >&2
        exit 1
    }
    if test "$canonical_source" != "$source_file" || test ! -f "$source_file" || test -L "$source_file"; then
        printf '%s\n' 'error: required OverCrow source must be canonical and regular' >&2
        exit 1
    fi
done

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(dirname -- "$script_dir")
destination="$repo_root/wit"
stage="$repo_root/.wit-sync.$$"
previous="$repo_root/.wit-previous.$$"

cleanup() {
    /usr/bin/rm -rf -- "$stage" "$previous"
}
trap cleanup EXIT HUP INT TERM

if test -e "$destination"; then
    if test ! -d "$destination" || test -L "$destination" \
            || test ! -f "$destination/widget-v1.wit" \
            || test -L "$destination/widget-v1.wit" \
            || test ! -f "$destination/widget-v1.sha256" \
            || test -L "$destination/widget-v1.sha256"; then
        printf '%s\n' 'error: existing WIT output is unsafe or incomplete' >&2
        exit 1
    fi
    current_hash=$(/usr/bin/sha256sum "$destination/widget-v1.wit")
    current_hash=${current_hash%% *}
    recorded_hash=$(/usr/bin/cat "$destination/widget-v1.sha256")
    if test "$current_hash" != "$recorded_hash"; then
        printf '%s\n' 'error: existing WIT output and checksum have drifted' >&2
        exit 1
    fi
fi

/usr/bin/install -d -m 0755 "$stage"
/usr/bin/install -m 0644 "$source_wit" "$stage/widget-v1.wit"
source_hash=$(/usr/bin/sha256sum "$stage/widget-v1.wit")
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
printf '%s\n' "$source_hash" >"$stage/widget-v1.sha256"

if test -e "$destination"; then
    /usr/bin/mv -- "$destination" "$previous"
fi
if ! /usr/bin/mv -- "$stage" "$destination"; then
    if test -e "$previous"; then
        /usr/bin/mv -- "$previous" "$destination"
    fi
    printf '%s\n' 'error: failed to publish synchronized WIT' >&2
    exit 1
fi
/usr/bin/rm -rf -- "$previous"
printf '%s\n' "$source_hash"
