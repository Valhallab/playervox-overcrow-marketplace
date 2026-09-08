#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: ci-admission-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(/usr/bin/mktemp -d /tmp/marketplace-ci-admission.XXXXXXXXXX)
cleanup() {
    /usr/bin/rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM

repository="$scratch/repository"
/usr/bin/git clone --quiet --no-hardlinks -- "$repo_root" "$repository"
/usr/bin/git -C "$repository" config user.name 'Marketplace CI Test'
/usr/bin/git -C "$repository" config user.email \
    'marketplace-ci-test@invalid.example'
# Keep admission fixtures independent of the released reference widget version.
fixture_version=1.2.3
node - "$repository/widgets/warframe-market/manifest.json" "$fixture_version" <<'JS'
const fs = require('node:fs');
const [manifestPath, version] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.version = version;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
JS
/usr/bin/git -C "$repository" add -- widgets/warframe-market/manifest.json
# During a red/green run these files may not be committed yet. Make the
# fixture's trusted revision contain the exact driver under test.
/usr/bin/install -D -m 0755 -- "$repo_root/scripts/ci-verify.sh" \
    "$repository/scripts/ci-verify.sh"
for relative in Cargo.lock Cargo.toml tools/marketplace-tool/Cargo.toml \
        tools/marketplace-tool/src/admission.rs \
        tools/marketplace-tool/src/catalog.rs \
        tools/marketplace-tool/src/main.rs tools/marketplace-tool/src/package.rs \
        tools/marketplace-tool/src/private_fs.rs \
        tools/marketplace-tool/src/snapshot.rs; do
    /usr/bin/install -D -m 0644 -- "$repo_root/$relative" \
        "$repository/$relative"
done
/usr/bin/git -C "$repository" add -- Cargo.lock Cargo.toml \
    scripts/ci-verify.sh tools/marketplace-tool/Cargo.toml \
    tools/marketplace-tool/src/admission.rs \
    tools/marketplace-tool/src/catalog.rs \
    tools/marketplace-tool/src/main.rs tools/marketplace-tool/src/package.rs \
    tools/marketplace-tool/src/private_fs.rs \
    tools/marketplace-tool/src/snapshot.rs
/usr/bin/git -C "$repository" commit --quiet --allow-empty \
    -m 'trusted driver fixture'
trust_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')

printf '%s\n' 'console.log("not declared by the manifest");' \
    >"$repository/widgets/warframe-market/undeclared.js"
/usr/bin/git -C "$repository" add -- \
    widgets/warframe-market/undeclared.js
/usr/bin/git -C "$repository" commit --quiet \
    -m 'invalid candidate fixture'
review_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')

private_parent="$scratch/private"
/usr/bin/install -d -m 0700 -- "$private_parent"
stdout="$scratch/stdout"
stderr="$scratch/stderr"
run_pull_request_admission() {
    sh "$repo_root/scripts/ci-verify.sh" \
        "$repository" "$trust_sha" "$1" pull_request \
        Valhallab/playervox-overcrow-marketplace candidate \
        contributor/playervox-overcrow-marketplace feature/widget \
        "$private_parent" admission
}

if run_pull_request_admission "$review_sha" >"$stdout" 2>"$stderr"; then
    printf '%s\n' \
        'error: CI admitted the trusted base instead of the exact candidate revision' >&2
    exit 1
fi

if ! /usr/bin/grep -F -x \
        'error: candidate artifact admission failed' "$stderr" >/dev/null; then
    printf '%s\n' 'error: candidate rejection was not explicit' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi
if test -s "$stdout"; then
    printf '%s\n' 'error: rejected admission produced a success receipt' >&2
    /usr/bin/cat "$stdout" >&2
    exit 1
fi

/usr/bin/git -C "$repository" checkout --quiet -B archive-fixture "$trust_sha"
printf '%s\n' 'widgets/warframe-market/undeclared.js export-ignore' \
    >"$repository/.gitattributes"
printf '%s\n' 'console.log("hidden from git archive");' \
    >"$repository/widgets/warframe-market/undeclared.js"
/usr/bin/git -C "$repository" add -- .gitattributes \
    widgets/warframe-market/undeclared.js
/usr/bin/git -C "$repository" commit --quiet \
    -m 'archive attribute attack fixture'
archive_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
if run_pull_request_admission "$archive_sha" >"$stdout" 2>"$stderr"; then
    printf '%s\n' 'error: CI accepted Git archive bytes that differ from the candidate tree' >&2
    exit 1
fi
if ! /usr/bin/grep -F -x \
        'error: candidate snapshot admission failed' "$stderr" >/dev/null \
        || test -s "$stdout"; then
    printf '%s\n' 'error: archive mismatch rejection was not explicit' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi

/usr/bin/git -C "$repository" checkout --quiet -B valid-fixture "$trust_sha"
printf '\n' >>"$repository/docs/creator-guide.md"
/usr/bin/git -C "$repository" add -- docs/creator-guide.md
/usr/bin/git -C "$repository" commit --quiet -m 'valid candidate fixture'
valid_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
valid_tree=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{tree}')
if ! run_pull_request_admission "$valid_sha" >"$stdout" 2>"$stderr"; then
    printf '%s\n' 'error: exact valid candidate revision was rejected' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi
expected_header="$scratch/expected-header"
printf 'admission\t2\t%s\t%s\t%s\n' \
    "$trust_sha" "$valid_sha" "$valid_tree" >"$expected_header"
if ! /usr/bin/grep -F -x -f "$expected_header" "$stdout" >/dev/null \
        || ! /usr/bin/awk -v expected_version="$fixture_version" -F '\t' '
            $1 == "artifact" \
                && $2 == "widgets/warframe-market" \
                && $3 == "com.playervox.overcrow.warframe.market" \
                && $4 == expected_version \
                && length($5) == 64 && $5 !~ /[^0-9a-f]/ \
                && $6 ~ /^[0-9]+$/ && $6 > 0 \
                && length($7) == 64 && $7 !~ /[^0-9a-f]/ \
                && $8 ~ /^[0-9]+$/ && $8 > 0 { found = 1 }
            END { exit found ? 0 : 1 }
        ' "$stdout" \
        || test "$(/usr/bin/tail -n 1 "$stdout")" \
            != 'Hosted static admission passed' \
        || test "$(/usr/bin/wc -l <"$stdout")" -ne 3 \
        || test -s "$stderr"; then
    printf '%s\n' 'error: exact candidate receipt is incomplete' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi

/usr/bin/git -C "$repository" checkout --quiet -B push-fixture "$trust_sha"
printf '%s\n' \
    'throw new Error("trusted push tests must execute");' \
    >"$repository/tests/site-runtime.test.js"
/usr/bin/git -C "$repository" add -- tests/site-runtime.test.js
/usr/bin/git -C "$repository" commit --quiet -m 'failing trusted push test'
push_sha=$(/usr/bin/git -C "$repository" rev-parse --verify 'HEAD^{commit}')
if (
    CDPATH='' cd -- "$repository"
    sh "$repository/scripts/ci-verify.sh" \
        "$repository" "$push_sha" "$push_sha" push \
        Valhallab/playervox-overcrow-marketplace candidate \
        Valhallab/playervox-overcrow-marketplace candidate \
        "$private_parent" admission
) >"$stdout" 2>"$stderr"; then
    printf '%s\n' 'error: CI skipped tests for the exact trusted push' >&2
    exit 1
fi
if ! /usr/bin/grep -F -x \
        'error: trusted source checks failed' "$stderr" >/dev/null; then
    printf '%s\n' 'error: trusted push failure was not explicit' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi

/usr/bin/git -C "$repository" checkout --quiet -B accepted-push "$trust_sha"
accepted_tree=$(/usr/bin/git -C "$repository" rev-parse --verify "$trust_sha^{tree}")
accepted_store="$scratch/accepted"
/usr/bin/install -d -m 0700 -- "$accepted_store"
if ! (
    CDPATH='' cd -- "$repository"
    sh "$repository/scripts/ci-verify.sh" \
        "$repository" "$trust_sha" "$trust_sha" push \
        Valhallab/playervox-overcrow-marketplace candidate \
        Valhallab/playervox-overcrow-marketplace candidate \
        "$private_parent" admission "$accepted_store"
) >"$stdout" 2>"$stderr"; then
    printf '%s\n' 'error: exact trusted admission was not persisted' >&2
    /usr/bin/cat "$stdout" >&2
    /usr/bin/cat "$stderr" >&2
    exit 1
fi
verified=$(cargo run -p marketplace-tool --locked --quiet -- \
    verify-admission --store "$accepted_store" --review-tree "$accepted_tree")
stored_listing=$(/usr/bin/find \
    "$accepted_store/listings/com.playervox.overcrow.warframe.market/$fixture_version" \
    -mindepth 1 -maxdepth 1 -type f -name '*.json' -print)
if test "$verified" != "$accepted_tree 1" \
        || test "$(printf '%s\n' "$stored_listing" | /usr/bin/wc -l)" -ne 1 \
        || ! /usr/bin/cmp -s -- \
            "$repository/widgets/warframe-market/listing.json" "$stored_listing" \
        || /usr/bin/find "$private_parent" -mindepth 1 -maxdepth 1 \
            -name 'verification.*' -print -quit | /usr/bin/grep . >/dev/null; then
    printf '%s\n' 'error: persisted admission is not independently verifiable' >&2
    exit 1
fi

printf '%s\n' 'CI exact-revision admission smoke tests passed'
