#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: verify-published-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
if test ! -f "$repo_root/scripts/verify-published.sh"; then
    printf '%s\n' 'error: published-tree verifier is unavailable' >&2
    exit 1
fi

# The Rust integration suite owns object mutation coverage. This smoke keeps
# the shell boundary causal: exact arguments, production identity, source and
# assembled landing checks, and fixed diagnostics for unsafe tree roots.
if sh "$repo_root/scripts/verify-published.sh" relative /missing/public.key \
        overcrow-production-2026-01 >"/tmp/verify-published-relative.$$" 2>&1; then
    /usr/bin/rm -f -- "/tmp/verify-published-relative.$$"
    printf '%s\n' 'error: relative published tree was accepted' >&2
    exit 1
fi
test "$(/usr/bin/cat "/tmp/verify-published-relative.$$")" = \
    'error: published tree rejected'
/usr/bin/rm -f -- "/tmp/verify-published-relative.$$"

for landing_test in tests/landing/effects.test.mjs \
        tests/landing/landing-content.test.mjs \
        tests/landing/static-hygiene.test.mjs; do
    node "$repo_root/$landing_test" "$repo_root/web/landing" >/dev/null
done

printf '%s\n' 'Published-tree verifier smoke tests passed'
