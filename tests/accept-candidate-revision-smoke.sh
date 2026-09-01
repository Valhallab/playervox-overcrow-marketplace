#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: accept-candidate-revision-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
accept_gate="$repo_root/scripts/accept-candidate-revision.sh"
bundle_tool="$repo_root/scripts/review-bundle.sh"
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
/usr/bin/cp -- "$bundle_tool" "$fixture/scripts/review-bundle.sh"
/usr/bin/chmod 0755 "$fixture/scripts/accept-candidate-revision.sh" \
    "$fixture/scripts/review-bundle.sh"
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
review_tree=$(/usr/bin/git -C "$fixture" rev-parse "$review_revision^{tree}")

/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"
printf '%s\n' reviewed >"$fixture/release.txt"
/usr/bin/git -C "$fixture" add release.txt
/usr/bin/git -C "$fixture" commit --quiet -m 'protected squash acceptance'
candidate_revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
test "$candidate_revision" != "$review_revision"
test "$(/usr/bin/git -C "$fixture" rev-parse "$candidate_revision^{tree}")" = \
    "$review_tree"
/usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
    "$candidate_revision"
/usr/bin/git -C "$fixture" checkout --quiet --detach "$trusted_base"

bundle_source="$scratch/reviewed-output"
bundle="$scratch/reviewed.bundle"
rejection_bundle="$scratch/rejection.bundle"
wrong_trust_bundle="$scratch/wrong-trust.bundle"
/usr/bin/install -d -m 0700 -- "$bundle_source/widgets/example"
printf '\000asmaccepted' >"$bundle_source/widgets/example/component.wasm"
/usr/bin/chmod 0644 "$bundle_source/widgets/example/component.wasm"
sh "$bundle_tool" create --source "$bundle_source" --output "$bundle" \
    --trust-sha "$trusted_base" --review-sha "$review_revision" \
    --review-tree "$review_tree"
sh "$bundle_tool" create --source "$bundle_source" --output "$rejection_bundle" \
    --trust-sha "$trusted_base" --review-sha "$review_revision" \
    --review-tree "$review_tree"
sh "$bundle_tool" create --source "$bundle_source" --output "$wrong_trust_bundle" \
    --trust-sha 4444444444444444444444444444444444444444 \
    --review-sha "$review_revision" --review-tree "$review_tree"

sh "$fixture/scripts/accept-candidate-revision.sh" \
    "$trusted_base" "$review_revision" "$candidate_revision" "$bundle"
sh "$bundle_tool" verify --bundle "$bundle" \
    --review-sha "$candidate_revision" --review-tree "$review_tree"
/usr/bin/grep -F -x "reviewRevision=$candidate_revision" \
    "$bundle/receipt" >/dev/null
printf '%s\n' 'case=rewritten-commit-identical-tree result=accepted-without-retest'

expect_rejection() {
    label=$1
    rejected_candidate=$2
    rejected_bundle=$3
    /usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
        "$rejected_candidate"
    if sh "$fixture/scripts/accept-candidate-revision.sh" \
            "$trusted_base" "$review_revision" "$rejected_candidate" \
            "$rejected_bundle" >"$scratch/$label.stdout" \
            2>"$scratch/$label.stderr"; then
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
expect_rejection tree-drift "$drifted_candidate" "$rejection_bundle"

unrelated_candidate=$(printf '%s\n' 'unrelated identical tree' \
    | /usr/bin/git -C "$fixture" commit-tree "$review_tree")
expect_rejection wrong-ancestry "$unrelated_candidate" "$rejection_bundle"

/usr/bin/git -C "$fixture" update-ref refs/remotes/origin/candidate \
    "$candidate_revision"
if sh "$fixture/scripts/accept-candidate-revision.sh" \
        "$trusted_base" "$review_revision" "$review_revision" "$rejection_bundle" \
        >"$scratch/stale-ref.stdout" 2>"$scratch/stale-ref.stderr"; then
    printf '%s\n' 'error: stale protected candidate ref was accepted' >&2
    exit 1
fi
printf '%s\n' 'case=stale-protected-ref result=rejected'

expect_rejection wrong-trusted-base "$candidate_revision" "$wrong_trust_bundle"

printf 'tampered' >>"$rejection_bundle/repository/widgets/example/component.wasm"
expect_rejection modified-bundle "$candidate_revision" "$rejection_bundle"

printf '%s\n' 'Accepted candidate revision smoke tests passed'
