#!/bin/sh
set -eu
LC_ALL=C
export LC_ALL

reject() {
    printf '%s\n' 'error: pull-request trusted-path policy rejected' >&2
    exit 1
}

for changed_path in "$@"; do
    case "$changed_path" in
        '' | -* | /* | */ | . | .. | ./* | ../* | */./* | */../* | */. | */.. | \
            *//* | *\\* | *[!A-Za-z0-9._+@/-]*) reject ;;
        .github | .github/* | scripts | scripts/* | tests | tests/* | \
            tools | tools/*) reject ;;
    esac
done
