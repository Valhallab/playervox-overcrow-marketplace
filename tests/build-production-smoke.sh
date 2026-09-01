#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: build-production-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
for required in scripts/build-production.sh scripts/verify-published.sh \
        scripts/prepare-marketplace-tool.sh scripts/review-bundle.sh; do
    if test ! -f "$repo_root/$required" || test -L "$repo_root/$required"; then
        printf '%s\n' 'error: production publisher is unavailable' >&2
        exit 1
    fi
done

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-production.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM

base="$scratch/base"
/usr/bin/install -d -m 0700 "$base"
for path in .gitignore Cargo.toml Cargo.lock rust-toolchain.toml marketplace \
        fixtures providers widgets sdk wit examples tools web scripts tests; do
    /usr/bin/cp -R -- "$repo_root/$path" "$base/"
done
/usr/bin/git init --quiet "$base"
/usr/bin/git -C "$base" config user.name 'Marketplace Tests'
/usr/bin/git -C "$base" config user.email 'marketplace-tests@invalid.example'
/usr/bin/git -C "$base" checkout --quiet -b release/fixture
/usr/bin/install -d -m 0755 "$base/published"
printf '%s\n' prior >"$base/published/prior.txt"

tool_work="$scratch/trusted-tool"
/usr/bin/install -d -m 0700 "$tool_work" "$base/keys"
trusted_tool=$(sh "$base/scripts/prepare-marketplace-tool.sh" "$base" "$tool_work")
fixture_seed=2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a
printf '%s\n' "$fixture_seed" >"$scratch/signing.key"
/usr/bin/chmod 0600 "$scratch/signing.key"
"$trusted_tool" derive-public-key --repository "$base" \
    --signing-key "$scratch/signing.key" \
    --key-id overcrow-production-2026-01 \
    --output "$base/keys/overcrow-production-2026-01.pub" >/dev/null
/usr/bin/git -C "$base" add --all
/usr/bin/git -C "$base" commit --quiet -m 'production fixture'
trust_revision=$(/usr/bin/git -C "$base" rev-parse HEAD)

reviewed_stage="$scratch/reviewed-stage"
sh "$base/scripts/stage-catalog-repository.sh" --mode production \
    --trusted-tool "$trusted_tool" "$reviewed_stage"

fixture="$scratch/release"
/usr/bin/git clone --quiet --no-hardlinks "$base" "$fixture"
/usr/bin/git -C "$fixture" checkout --quiet release/fixture
/usr/bin/chmod 0644 "$fixture/keys/overcrow-production-2026-01.pub"
restage_marker="$scratch/production-restaged"
printf '%s\n' \
    '#!/bin/sh' \
    "printf '%s\\n' restaged >'$restage_marker'" \
    'exit 99' >"$fixture/scripts/stage-catalog-repository.sh"
/usr/bin/chmod 0755 "$fixture/scripts/stage-catalog-repository.sh"
/usr/bin/git -C "$fixture" add scripts/stage-catalog-repository.sh
/usr/bin/git -C "$fixture" commit --quiet -m 'reviewed no-restage canary'
candidate_revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
candidate_tree=$(/usr/bin/git -C "$fixture" rev-parse "$candidate_revision^{tree}")

reviewed_source="$scratch/reviewed-source"
/usr/bin/cp -R -- "$reviewed_stage" "$reviewed_source"
/usr/bin/install -m 0700 -- "$fixture/scripts/stage-catalog-repository.sh" \
    "$reviewed_source/scripts/stage-catalog-repository.sh"
bundle="$scratch/reviewed.bundle"
sh "$repo_root/scripts/review-bundle.sh" create \
    --source "$reviewed_source" --output "$bundle" \
    --trust-sha "$trust_revision" --review-sha "$candidate_revision" \
    --review-tree "$candidate_tree"

secrets="$scratch/authority"
/usr/bin/install -d -m 0700 "$secrets"
printf '%s\n' 1 >"$secrets/sequence.txt"
printf '%s\n' "$fixture_seed" >"$secrets/signing.key"
/usr/bin/chmod 0600 "$secrets/sequence.txt" "$secrets/signing.key"

snapshot_published() {
    output=$1
    /usr/bin/find "$fixture/published" -xdev -type f -printf '%P\n' \
        | LC_ALL=C /usr/bin/sort \
        | while IFS= read -r relative; do
            digest=$(/usr/bin/sha256sum "$fixture/published/$relative" \
                | /usr/bin/cut -d ' ' -f 1)
            printf '%s\t%s\n' "$relative" "$digest"
        done >"$output"
}

run_publisher() (
    selected_bundle=$1
    stdout=$2
    stderr=$3
    cd "$fixture"
    PATH="$scratch/fake-path:$PATH" sh scripts/build-production.sh \
        --candidate-revision "$candidate_revision" \
        --review-bundle "$selected_bundle" \
        --sequence-file "$secrets/sequence.txt" \
        --sequence-state "$secrets/state.json" \
        --signing-key "$secrets/signing.key" \
        --public-key "$fixture/keys/overcrow-production-2026-01.pub" \
        --key-id overcrow-production-2026-01 \
        >"$stdout" 2>"$stderr"
)

/usr/bin/install -d -m 0700 "$scratch/fake-path"
printf '%s\n' '#!/bin/sh' \
    "printf '%s\\n' ran >'$scratch/ambient-cargo-ran'" \
    'exit 99' >"$scratch/fake-path/cargo"
/usr/bin/chmod 0700 "$scratch/fake-path/cargo"

before="$scratch/published.before"
snapshot_published "$before"
tampered_bundle="$scratch/tampered.bundle"
/usr/bin/cp -a -- "$bundle" "$tampered_bundle"
tampered_component=$(/usr/bin/find "$tampered_bundle/repository" \
    -type f -name component.wasm -print -quit)
printf 'tampered' >>"$tampered_component"
if run_publisher "$tampered_bundle" "$scratch/tampered.stdout" \
        "$scratch/tampered.stderr"; then
    printf '%s\n' 'error: modified review bundle was published' >&2
    exit 1
fi
test ! -s "$scratch/tampered.stdout"
test "$(/usr/bin/cat "$scratch/tampered.stderr")" = \
    'error: production review bundle rejected'
after="$scratch/published.after"
snapshot_published "$after"
/usr/bin/cmp --silent "$before" "$after"
test "$(/usr/bin/cat "$secrets/sequence.txt")" = 1

run_publisher "$bundle" "$scratch/success.stdout" "$scratch/success.stderr"
test ! -s "$scratch/success.stdout" && test ! -s "$scratch/success.stderr"
test ! -e "$restage_marker" && test ! -L "$restage_marker"
test ! -e "$scratch/ambient-cargo-ran" && test ! -L "$scratch/ambient-cargo-ran"
test "$(/usr/bin/cat "$secrets/sequence.txt")" = 2
test -f "$secrets/state.json"
test ! -e "$secrets/state.json.receipt" && test ! -L "$secrets/state.json.receipt"
test -f "$fixture/published/index.html"
test -f "$fixture/published/marketplace/index.html"
test -f "$fixture/published/marketplace/v1/catalog.json"
test ! -e "$fixture/published/marketplace/policies"

sh "$fixture/scripts/verify-published.sh" "$fixture/published" \
    "$fixture/keys/overcrow-production-2026-01.pub" \
    overcrow-production-2026-01 >/dev/null

printf '%s\n' 'Production publisher smoke tests passed'
