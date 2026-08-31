#!/bin/sh
set -eu
umask 077

if test "$#" -ne 3; then
    printf '%s\n' \
        'usage: verify-deployment.sh ABSOLUTE-PUBLISHED-TREE ABSOLUTE-PUBLIC-KEY overcrow-production-2026-01' >&2
    exit 2
fi

expected_tree=$1
public_key=$2
key_id=$3
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=${script_dir%/scripts}
unset MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT
# shellcheck disable=SC2034 # Consumed by the sourced verifier library.
MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT=production-wrapper
# shellcheck source=/dev/null
. "$script_dir/verify-deployment-lib.sh"

verify_marketplace_production_deployment "$repo_root" \
    https://overcrow.playervox.com "$expected_tree" "$public_key" "$key_id"
