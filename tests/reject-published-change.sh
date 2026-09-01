#!/bin/sh
set -eu
LC_ALL=C
export LC_ALL

reject() {
    printf '%s\n' 'error: pull-request publication policy rejected' >&2
    exit 1
}

if test "$#" -lt 5; then
    printf '%s\n' \
        'usage: reject-published-change.sh EVENT REPOSITORY BASE HEAD-REPOSITORY HEAD [PATH ...]' >&2
    exit 2
fi

event_name=$1
repository=$2
base_ref=$3
head_repository=$4
head_ref=$5
shift 5

case "$event_name" in
    pull_request) ;;
    *) reject ;;
esac

for repository_name in "$repository" "$head_repository"; do
    case "$repository_name" in
        '' | /* | */ | */*/* | *..* | *[!A-Za-z0-9._/-]*) reject ;;
    esac
done

case "$base_ref" in
    master | candidate) ;;
    *) reject ;;
esac
case "$head_ref" in
    '' | -* | /* | */ | *//* | *..* | *@\{* | *\\* | *[!A-Za-z0-9._/-]*) reject ;;
esac

published_changed=false
for changed_path in "$@"; do
    case "$changed_path" in
        '' | -* | /* | */ | . | .. | ./* | ../* | */./* | */../* | */. | */.. | \
            *//* | *\\* | *[!A-Za-z0-9._+@/-]*) reject ;;
        published | published/*) published_changed=true ;;
    esac
done

if test "$published_changed" = false; then
    exit 0
fi
if test "$base_ref" != master || test "$head_repository" != "$repository"; then
    reject
fi

case "$head_ref" in
    release/*)
        release_name=${head_ref#release/}
        case "$release_name" in
            '' | -* | */* | *[!A-Za-z0-9._-]*) reject ;;
        esac
        ;;
    *) reject ;;
esac
