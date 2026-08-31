#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: accept-candidate-revision-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
accept_gate="$repo_root/scripts/accept-candidate-revision.sh"
if test ! -f "$accept_gate" || test -L "$accept_gate"; then
    printf '%s\n' 'error: accepted-revision gate is unavailable' >&2
    exit 1
fi

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-accepted-revision.XXXXXXXXXX)
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$scratch"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

fixture="$scratch/repository"
/usr/bin/install -d -m 0700 -- "$fixture/scripts"
/usr/bin/cp -- "$accept_gate" "$fixture/scripts/accept-candidate-revision.sh"
gate_log="$scratch/review-gate.log"
# shellcheck disable=SC2016 # The generated fixture expands these variables.
printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'test "$#" -eq 2' \
    'printf "%s %s\\n" "$1" "$2" >>"$ACCEPTANCE_GATE_LOG"' \
    >"$fixture/scripts/review-revision.sh"
/usr/bin/chmod 0755 "$fixture/scripts/accept-candidate-revision.sh" \
    "$fixture/scripts/review-revision.sh"
printf '%s\n' base >"$fixture/release.txt"
/usr/bin/git init --quiet "$fixture"
/usr/bin/git -C "$fixture" config user.name 'Marketplace Acceptance Tests'
/usr/bin/git -C "$fixture" config user.email 'acceptance-tests@invalid.example'
/usr/bin/git -C "$fixture" add --all
/usr/bin/git -C "$fixture" commit --quiet -m 'trusted protected base'
trusted_base=$(/usr/bin/git -C "$fixture" rev-parse HEAD)

/usr/bin/git -C "$fixture" checkout --quiet -b reviewed-head
printf '%s\n' reviewed >"$fixture/release.txt"
/usr/bin/git -C "$fixture" add release.txt
/usr/bin/git -C "$fixture" commit --quiet -m 'reviewed pull request head'
review_revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)

/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"
printf '%s\n' reviewed >"$fixture/release.txt"
/usr/bin/git -C "$fixture" add release.txt
/usr/bin/git -C "$fixture" commit --quiet -m 'protected squash acceptance'
candidate_revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
test "$candidate_revision" != "$review_revision"
test "$(/usr/bin/git -C "$fixture" rev-parse "$candidate_revision^{tree}")" = \
    "$(/usr/bin/git -C "$fixture" rev-parse "$review_revision^{tree}")"
/usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
    "$candidate_revision"
/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"

ACCEPTANCE_GATE_LOG=$gate_log \
    sh "$fixture/scripts/accept-candidate-revision.sh" \
        "$trusted_base" "$review_revision" "$candidate_revision"
test "$(/usr/bin/cat "$gate_log")" = "$trusted_base $candidate_revision"
printf '%s\n' 'case=rewritten-commit-identical-tree result=accepted-and-gated'

expect_rejection() {
    label=$1
    rejected_candidate=$2
    /usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
        "$rejected_candidate"
    if ACCEPTANCE_GATE_LOG=$gate_log \
            sh "$fixture/scripts/accept-candidate-revision.sh" \
                "$trusted_base" "$review_revision" "$rejected_candidate" \
                >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        printf '%s\n' "error: $label accepted-revision defect passed" >&2
        exit 1
    fi
    printf '%s\n' "case=$label result=rejected"
}

/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"
printf '%s\n' drifted >"$fixture/release.txt"
/usr/bin/git -C "$fixture" add release.txt
/usr/bin/git -C "$fixture" commit --quiet -m 'unreviewed accepted bytes'
drifted_candidate=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"
expect_rejection tree-drift "$drifted_candidate"

review_tree=$(/usr/bin/git -C "$fixture" rev-parse "$review_revision^{tree}")
unrelated_candidate=$(printf '%s\n' 'unrelated identical tree' \
    | /usr/bin/git -C "$fixture" commit-tree "$review_tree")
expect_rejection wrong-ancestry "$unrelated_candidate"

/usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
    "$candidate_revision"
if ACCEPTANCE_GATE_LOG=$gate_log \
        sh "$fixture/scripts/accept-candidate-revision.sh" \
            "$trusted_base" "$review_revision" "$review_revision" \
            >"$scratch/stale-ref.stdout" 2>"$scratch/stale-ref.stderr"; then
    printf '%s\n' 'error: stale protected candidate ref was accepted' >&2
    exit 1
fi
printf '%s\n' 'case=stale-protected-ref result=rejected'

test "$(/usr/bin/wc -l <"$gate_log")" -eq 1
printf '%s\n' 'Accepted candidate revision smoke tests passed'
