#!/bin/sh
set -eu
repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
fixture="$repo_root/.policy-secret-fixture"
cleanup() { /usr/bin/rm -f -- "$fixture"; }
trap cleanup EXIT HUP INT TERM
printf '%s\\n' '-----BEGIN PRIVATE '"KEY-----" >"$fixture"
if "$repo_root/scripts/check-policy.sh"; then exit 1; fi
printf '%s\\n' 'g'"hp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN" >"$fixture"
if "$repo_root/scripts/check-policy.sh"; then exit 1; fi
