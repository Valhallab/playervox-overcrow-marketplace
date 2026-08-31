# Production Marketplace Operations

This runbook is the sole operational source of truth for production
publication. It defines preparation and review of the public trust change; it
does not authorize a push, deployment, or publication.

## 1. Preconditions and role separation

Use separate clean worktrees and roles: contributors submit candidate PRs;
hosted CI performs static admission; a maintainer reviews the exact revision
and runs the complete networkless gate; acceptance merges only to `candidate`;
an offline publisher creates the release snapshot; and a separate deployment
operator configures Coolify to serve tracked output. Coolify, GitHub, CI, and
project temporary files never receive production authority material.

Use named values, never real authority paths in a commit, issue, or PR:

```sh
marketplace_root=/absolute/path/to/a/clean/release-worktree
candidate_root=/absolute/path/to/a/clean/candidate-review-worktree
trust_root=/absolute/path/to/a/clean/marketplace-trust-worktree
authority_root=/absolute/private/path/outside/all/repositories
candidate_revision=40-lowercase-hex-characters
trust_revision=40-lowercase-hex-characters
trusted_base_revision=40-lowercase-hex-characters
review_revision=40-lowercase-hex-characters
sequence_name=1
overcrow_public_key=/absolute/path/to/the/reviewed/OverCrow-production-key-file
```

`review_revision` is the exact pre-merge pull-request head approved by the
maintainer. `candidate_revision` is the distinct protected post-merge
`candidate` revision accepted by GitHub; its commit identity may change during
a compliant squash or rebase merge, but its fully resolved Git tree must not.

The fixed production origin is
`https://overcrow.playervox.com/marketplace/v1/`. A production catalog is valid
for exactly 90 days: republish by day 60, on every content change, and
immediately for a signed security suspension or revocation. An older sequence
is never republished as a rollback.

## 2. Repository visibility and GitHub rulesets

The repository must remain public for public static hosting, but treat the
repository and every pull request as untrusted publication inputs. Verify that
invariant through the GitHub interface; stop if it cannot be established.
Apply these two distinct rules to both `candidate` and `master`:

```text
Technical rule: require pull request; require strict current-head checks
`verify` plus `overcrow/marketplace-admission/<base>`; require linear history;
block force-push and deletion; apply to administrators; no bypass actor.

Human-review rule: require one approval; dismiss stale approvals; require
approval of the latest reviewable push; prevent the last pusher from approving;
grant only @ypMrg a pull-request-scoped bypass. Remove that bypass when a
second trusted maintainer exists.
```

For `candidate`, `<base>` is `candidate`; for `master`, it is `master`.
`verify` is pinned to the GitHub Actions app. The base-specific admission
result is a separately required commit-status context, not a GitHub App check.
If the configured controls cannot establish this contract, stop; do not infer
GitHub setting names or substitute a weaker configuration.

## 3. Offline authority directory and key creation

Do this on the offline authority host, outside every repository, CI workspace,
Coolify volume, and project temporary directory. No production key currently
exists. The exact key ID is `overcrow-production-2026-01`. Record
`trust_revision` from the separately reviewed marketplace trust checkout; do
not build authority tooling until that checkout is still exact and clean.

```sh
signing_key="$authority_root/overcrow-production-2026-01.key"
sequence_file="$authority_root/sequence.txt"
sequence_state="$authority_root/sequence-state.json"
derived_public_key="$authority_root/overcrow-production-2026-01.pub"

set -eu
case "$trust_revision" in *[!0-9a-f]* | '') exit 1 ;; esac
test "${#trust_revision}" -eq 40
test ! -L "$trust_root"
test "$(CDPATH='' cd -- "$trust_root" && pwd -P)" = "$trust_root"
test "$(/usr/bin/git -C "$trust_root" rev-parse --verify "$trust_revision^{commit}")" = "$trust_revision"
test "$(/usr/bin/git -C "$trust_root" rev-parse HEAD)" = "$trust_revision"
test -z "$(/usr/bin/git -C "$trust_root" status --porcelain=v1 --untracked-files=normal)"

umask 077
/usr/bin/install -d -m 0700 -- "$authority_root"
receipt="$sequence_state.receipt"
for authority_target in "$signing_key" "$sequence_file" "$sequence_state" "$receipt" "$derived_public_key"; do
  test ! -e "$authority_target" && test ! -L "$authority_target"
done
(set -C; : >"$signing_key")
(set -C; : >"$sequence_file")
/usr/bin/openssl rand -hex 32 >"$signing_key"
printf '%s\n' 1 >"$sequence_file"
test "$(/usr/bin/stat -c '%a' "$authority_root")" = 700
test "$(/usr/bin/stat -c '%a' "$signing_key")" = 600
test "$(/usr/bin/stat -c '%a' "$sequence_file")" = 600
test "$(/usr/bin/wc -c <"$signing_key")" -eq 65

initial_tool_work="$authority_root/tool-initial"
test ! -e "$initial_tool_work" && test ! -L "$initial_tool_work"
/usr/bin/install -d -m 0700 -- "$initial_tool_work"
marketplace_tool=$(sh "$trust_root/scripts/prepare-marketplace-tool.sh" \
  "$trust_root" "$initial_tool_work")
"$marketplace_tool" derive-public-key --repository "$trust_root" \
  --signing-key "$signing_key" --key-id overcrow-production-2026-01 \
  --output "$derived_public_key" >/dev/null
test "$(/usr/bin/wc -c <"$derived_public_key")" -eq 65
/usr/bin/grep -Eq '^[0-9a-f]{64}$' "$derived_public_key"
```

Leave `sequence_state` absent until the first successful publisher run creates
it with mode `0600`; its receipt is also private. The signing key, counter,
state, receipt, and every authority path remain outside repositories, GitHub,
CI, Coolify, logs, and project temporary files. The commands above redirect
private bytes directly to a mode-`0600` file and never print them.

## 4. Private recovery backup

Before public-key activation, create an encrypted recovery backup under private
operator control and test it through the restore verification below. The
provider, account, device, location, labels, and recovery mechanism are private
operator information and must not be recorded in a repository, issue, pull
request, CI output, deployment system, or public runbook.

Do not proceed if the required backup is absent, inaccessible, or has not
passed restore verification.

## 5. Restore verification

Restore the required backup into a fresh private authority directory, then
derive its public key without exposing private bytes. Build the trusted tool in
that private directory and keep the command quiet:

```sh
restore_root=/absolute/private/path/outside/all/repositories/restored-authority
restore_public_key="$restore_root/restored.pub"
set -eu
/usr/bin/install -d -m 0700 -- "$restore_root"
restore_tool_work="$restore_root/tool"
test ! -e "$restore_tool_work" && test ! -L "$restore_tool_work"
/usr/bin/install -d -m 0700 -- "$restore_tool_work"

test ! -L "$trust_root"
test "$(CDPATH='' cd -- "$trust_root" && pwd -P)" = "$trust_root"
test "$(/usr/bin/git -C "$trust_root" rev-parse --verify "$trust_revision^{commit}")" = "$trust_revision"
test "$(/usr/bin/git -C "$trust_root" rev-parse HEAD)" = "$trust_revision"
test -z "$(/usr/bin/git -C "$trust_root" status --porcelain=v1 --untracked-files=normal)"
marketplace_tool=$(sh "$trust_root/scripts/prepare-marketplace-tool.sh" \
  "$trust_root" "$restore_tool_work")
"$marketplace_tool" derive-public-key --repository "$trust_root" \
  --signing-key "$restore_root/overcrow-production-2026-01.key" \
  --key-id overcrow-production-2026-01 --output "$restore_public_key" >/dev/null
/usr/bin/cmp --silent "$restore_public_key" "$derived_public_key"
```

A failed or non-identical `cmp` stops activation. Repeat the check for every
retained encrypted recovery copy. Never use `cat`, a debugger, a log, or a
command substitution to display private key bytes.

## 6. Public-key review and activation

The initial public key was derived and validated privately before backup. Review
it as ordinary public source, but activate it only after the required restore
test:

```sh
set -eu
/usr/bin/install -d -m 0755 -- "$trust_root/keys"
/usr/bin/install -m 0644 -- "$derived_public_key" \
  "$trust_root/keys/overcrow-production-2026-01.pub"
/usr/bin/install -m 0644 -- "$derived_public_key" "$overcrow_public_key"
/usr/bin/cmp --silent "$trust_root/keys/overcrow-production-2026-01.pub" \
  "$overcrow_public_key"
```

The marketplace and OverCrow files must be identical 65-byte,
lowercase-hex-plus-newline files. Review and commit the public key in both
repositories, then activate production trust only after the required backup
restore has passed. Do not record the authority path, backup location, provider,
account, or recovery secret in either repository, a PR, an issue, CI, or a
deployment system.

## 7. Acceptance, release branch, offline signing, and master PR

Record the exact trusted candidate-base SHA and proposed review SHA before
human review. From a clean checkout at the trusted base, run the repository's
review wrapper; it prepares pinned dependencies from the trusted snapshot, then
runs candidate compilation and tests in the bounded networkless sandbox:

```sh
set -eu
case "$sequence_name" in 0 | 0* | *[!0-9]* | '') exit 1 ;; esac
test "${#sequence_name}" -le 16
test "$sequence_name" -lt 9007199254740991
review_private_parent="$authority_root/review-$sequence_name"
/usr/bin/install -d -m 0700 -- "$review_private_parent"
test "$(git -C "$candidate_root" rev-parse HEAD)" = "$trusted_base_revision"
test -z "$(git -C "$candidate_root" status --porcelain=v1 --untracked-files=normal)"
test "$trusted_base_revision" != "$review_revision"
(
  cd "$candidate_root"
  sh scripts/review-revision.sh "$trusted_base_revision" "$review_revision"
)
```

Merge acceptance only after that gate passes. The protected merge may rewrite
the commit identity. Refresh and record the protected post-merge
`candidate_revision`, require its tree to equal the reviewed PR-head tree and
its merge base to be the prior protected `trusted_base_revision`, then rerun
the complete gate against that exact accepted revision. Only after the second
gate passes may the release name be bound to the private counter without
printing it and a release branch be created:

```sh
set -eu
accepted_git() {
  /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
    GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
    /usr/bin/timeout --signal=KILL 30 \
    /usr/bin/prlimit --cpu=20 --as=1073741824 --nofile=128 \
      --fsize=33554432 -- \
    /usr/bin/git --no-replace-objects \
      -c core.fsmonitor=false -c core.hooksPath=/dev/null \
      -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
      -c commit.gpgSign=false -c diff.external= -C "$candidate_root" "$@"
}
test "$(accepted_git remote get-url origin)" = \
  https://github.com/Valhallab/playervox-overcrow-marketplace.git
accepted_git fetch --no-tags --no-write-fetch-head --force origin \
  refs/heads/candidate:refs/remotes/origin/candidate
candidate_revision=$(accepted_git rev-parse --verify \
  refs/remotes/origin/candidate^{commit})
case "$candidate_revision" in '' | *[!0-9a-f]*) exit 1 ;; esac
test "${#candidate_revision}" -eq 40
(
  cd "$candidate_root"
  sh scripts/accept-candidate-revision.sh "$trusted_base_revision" \
    "$review_revision" "$candidate_revision"
)
sequence_owner=$(/usr/bin/id -u)
test -f "$sequence_file" && test ! -L "$sequence_file"
sequence_bytes=$(/usr/bin/wc -c <"$sequence_file")
case "$sequence_bytes" in '' | *[!0-9]*) exit 1 ;; esac
test "$sequence_bytes" -ge 2 && test "$sequence_bytes" -le 17
test "$(/usr/bin/stat -c '%u:%a:%h:%s' "$sequence_file")" = \
  "$sequence_owner:600:1:$sequence_bytes"
test "$(/usr/bin/wc -l <"$sequence_file")" -eq 1
/usr/bin/grep -Eq '^[1-9][0-9]{0,15}$' "$sequence_file" >/dev/null
IFS= read -r recorded_sequence <"$sequence_file"
test "$sequence_bytes" -eq "$((${#recorded_sequence} + 1))"
test "$recorded_sequence" = "$sequence_name"
accepted_git worktree add -b "release/$sequence_name" \
  "$marketplace_root" "$candidate_revision"
test "$(git -C "$marketplace_root" rev-parse HEAD)" = "$candidate_revision"
test "$(git -C "$marketplace_root" branch --show-current)" = "release/$sequence_name"
test -z "$(git -C "$marketplace_root" status --porcelain=v1 --untracked-files=normal)"
```

On the offline authority host, verify the key privately, sign the exact clean
release tree, and verify the generated tracked tree before opening the
`release/<sequence>` PR to `master`:

```sh
set -eu
release_tool_work="$authority_root/tool-release-$sequence_name"
test ! -e "$release_tool_work" && test ! -L "$release_tool_work"
/usr/bin/install -d -m 0700 -- "$release_tool_work"
marketplace_tool=$(sh "$marketplace_root/scripts/prepare-marketplace-tool.sh" \
  "$marketplace_root" "$release_tool_work")
"$marketplace_tool" verify-signing-key --repository "$marketplace_root" \
  --signing-key "$signing_key" --key-id overcrow-production-2026-01 >/dev/null
(
  cd "$marketplace_root"
  sh scripts/build-production.sh --candidate-revision "$candidate_revision" \
    --sequence-file "$sequence_file" --sequence-state "$sequence_state" \
    --signing-key "$signing_key" \
    --public-key "$marketplace_root/keys/overcrow-production-2026-01.pub" \
    --key-id overcrow-production-2026-01
)
sh "$marketplace_root/scripts/verify-published.sh" "$marketplace_root/published" \
  "$marketplace_root/keys/overcrow-production-2026-01.pub" \
  overcrow-production-2026-01
```

The publisher atomically replaces `published/`, advances the private sequence,
and does not deploy or push. Review the resulting release diff and use a PR to
`master`; do not copy an artifact into Coolify or sign from CI.

## 8. Coolify static deployment

Configure a static deployment with repository
`Valhallab/playervox-overcrow-marketplace`, branch `master`, and publish/base
directory `published`. Set no build command, no secret, and no persistent
publisher state. Use only the HTTPS custom domain
`overcrow.playervox.com`. Coolify serves tracked `published/` after the master
PR; it does not build, sign, sequence, or promote catalog data.

Stop if the configuration requires a credential, a build step, a writable
publisher volume, HTTP-only access, a different branch, or any source outside
tracked `published/`.

## 9. Response cache and MIME policy

Landing and browser assets may revalidate normally but must not be treated as
immutable catalog authority. Serve `catalog.json` and every response enclosing
its signed metadata with `Cache-Control: no-cache`: storage is allowed, but
revalidation is required. Serve digest-named package and preview objects with
`Cache-Control: public, max-age=31536000, immutable`.

Preserve appropriate MIME types: JSON for catalog data, JavaScript for scripts,
CSS for stylesheets, PNG for previews, and `application/octet-stream` for
package objects. Do not use a redirect to achieve cache routing or content
typing.

## 10. Public endpoint checks

After an explicitly authorized deployment, use the bounded reviewed checker.
It disables curl configuration, accepts HTTPS at the fixed origin only, rejects
every redirect, compares the deployed tree and catalog sequence to the already
verified tracked release, verifies the signature/key/90-day validity, and
checks every package and preview byte count, digest, cache header, and MIME
type without parsing untrusted catalog data in shell:

```sh
set -eu
sh "$marketplace_root/scripts/verify-deployment.sh" \
  "$marketplace_root/published" \
  "$marketplace_root/keys/overcrow-production-2026-01.pub" \
  overcrow-production-2026-01
```

The checker also requires direct `200` responses for `/`, `/marketplace/`, and
`/marketplace/v1/catalog.json`. Stop on missing HTTPS, any redirect, an invalid
signature, an unknown key ID, expiry, stale or older sequence, an object
mismatch, or an incorrect cache or MIME response.

## 11. Rotation, loss, compromise, suspension, and rollback

For planned rotation, create and restore-test a new offline key first. Release
OverCrow with both old and new public keys, continue signing with the old key
until compatible clients exist, switch publication to the new key, then remove
the old trust identity only after the transition and old-catalog validity
windows pass.

For irretrievable loss without compromise, stop publication, create and
restore-test a replacement key, release OverCrow trust for it, then resume.
For suspected compromise, stop publication immediately, treat it as a security
incident, audit affected catalog history, release clients that remove the old
key and trust the replacement, and never use an unsigned recovery path.

For a package incident, publish a signed suspension or revocation immediately
in a higher sequence. A corrective rollback is likewise a newly signed,
higher-sequence catalog, even when it restores older package content. Never
redeploy an older catalog sequence.

## 12. Stop conditions

Stop and investigate rather than bypassing a control when the required recovery
backup is missing; restored or repository public keys differ; a checkout is
dirty; the branch or revision is wrong; the gate fails; the sequence is stale
or older; HTTPS is missing; a redirect is unexpected; or any secret or private
publication-authority material is in Coolify or GitHub. The same stop applies
to private key bytes, recovery secrets, counters, receipts, or authority paths
in a repository, issue, PR, CI output, log, artifact, or project temporary
file.
