#!/bin/sh
set -eu

if test "$#" -ne 0; then
    printf '%s\n' 'usage: check-policy.sh' >&2
    exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -L)
physical_script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
if test "$script_dir" != "$physical_script_dir"; then
    printf '%s\n' 'error: repository path must not contain symlinks' >&2
    exit 1
fi
repo_root=$(dirname -- "$physical_script_dir")

command -v git >/dev/null 2>&1 || {
    printf '%s\n' 'error: git is required' >&2
    exit 1
}
command -v rg >/dev/null 2>&1 || {
    printf '%s\n' 'error: ripgrep is required' >&2
    exit 1
}

git_root=$(git -C "$repo_root" rev-parse --show-toplevel 2>/dev/null) || {
    printf '%s\n' 'error: repository root is unavailable' >&2
    exit 1
}
git_root=$(CDPATH='' cd -- "$git_root" && pwd -P)
if test "$git_root" != "$repo_root"; then
    printf '%s\n' 'error: script is not running from its repository' >&2
    exit 1
fi

for required in \
        .gitignore Cargo.toml rust-toolchain.toml README.md LICENSE \
        TRADEMARKS.md CONTRIBUTING.md SECURITY.md \
        docs/review-policy.md docs/permissions.md docs/localization.md \
        docs/publishing.md scripts/check-policy.sh; do
    if ! test -f "$repo_root/$required" || test -L "$repo_root/$required"; then
        printf '%s\n' "error: missing or symlinked policy file: $required" >&2
        exit 1
    fi
done

if rg --hidden --no-ignore -n 'BEGIN (RSA|OPENSSH|EC) PRIVATE KEY' "$repo_root" \
        --glob '!.git/**' \
        --glob '!fixtures/keys/development-ed25519.key'; then
    printf '%s\n' 'error: private key material is not allowed' >&2
    exit 1
else
    scan_status=$?
fi
if test "$scan_status" -ne 1; then
    printf '%s\n' 'error: private-key scan failed' >&2
    exit 1
fi

printf '%s\n' 'Marketplace policy checks passed'
