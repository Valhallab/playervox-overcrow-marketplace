#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: release-snapshot-gate-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
gate_relative=scripts/verify-release-snapshot.sh
if test ! -x "$repo_root/$gate_relative" || test -L "$repo_root/$gate_relative"; then
    printf '%s\n' 'error: trusted release-snapshot gate is unavailable' >&2
    exit 1
fi

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-release-gate.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM

tool_work="$scratch/tool"
/usr/bin/install -d -m 0700 -- "$tool_work"
trusted_tool=$(sh "$repo_root/scripts/prepare-marketplace-tool.sh" \
    "$repo_root" "$tool_work")
# Node only constructs deliberately malformed signed test fixtures. It is not
# part of the trusted release-gate path exercised below.
node_path=/usr/bin/node
if test ! -x "$node_path" || test -L "$node_path"; then
    printf '%s\n' 'error: fixture Node is unavailable' >&2
    exit 1
fi

template="$scratch/template"
/usr/bin/install -d -m 0700 -- "$template"
for path in .github .gitignore CONTRIBUTING.md LICENSE README.md SECURITY.md \
        TRADEMARKS.md Cargo.toml Cargo.lock rust-toolchain.toml docs examples \
        fixtures marketplace providers sdk tools widgets wit scripts tests web; do
    /usr/bin/cp -R -- "$repo_root/$path" "$template/"
done
printf '%s\n' \
    '[' \
    '  {' \
    '    "sourceDirectory": "examples/hello-widget",' \
    '    "cargoPackage": "hello-widget",' \
    '    "componentArtifact": "hello_widget",' \
    '    "status": "verified"' \
    '  }' \
    ']' >"$template/marketplace/targets.json"
/usr/bin/install -d -m 0755 -- \
    "$template/keys" "$template/public/marketplace/v1/packages" \
    "$template/public/marketplace/v1/previews"
/usr/bin/install -m 0644 -- "$template/web/landing/index.html" \
    "$template/public/index.html"

# Build one reviewed fixture component so Git-backed positive snapshots contain
# real package and preview objects. The release gate still treats those bytes
# only as signed data and never executes them.
fixture_target="$scratch/fixture-target"
if ! /usr/bin/timeout --signal=TERM --kill-after=5 180 \
        cargo build --manifest-path "$repo_root/Cargo.toml" \
            --package hello-widget --release --target wasm32-wasip2 \
            --target-dir "$fixture_target" --locked --offline --quiet; then
    printf '%s\n' 'error: fixture component build failed' >&2
    exit 1
fi
/usr/bin/install -m 0644 -- \
    "$fixture_target/wasm32-wasip2/release/hello_widget.wasm" \
    "$template/examples/hello-widget/component.wasm"
component_sha=$(/usr/bin/sha256sum \
    "$template/examples/hello-widget/component.wasm" | /usr/bin/cut -d ' ' -f 1)
"$node_path" -e '
const fs = require("node:fs");
const [path, digest] = process.argv.slice(1);
const manifest = JSON.parse(fs.readFileSync(path, "utf8"));
manifest.files.component.sha256 = digest;
fs.writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
' "$template/examples/hello-widget/manifest.json" "$component_sha"
/usr/bin/find "$template/examples/hello-widget" -type d \
    -exec /usr/bin/chmod 0755 -- {} +
/usr/bin/find "$template/examples/hello-widget" -type f \
    -exec /usr/bin/chmod 0644 -- {} +
/usr/bin/chmod 0644 -- "$template/marketplace/targets.json"

production_seed=2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a
attacker_seed=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
generated_at=$(/usr/bin/date -u -d '1 day ago' '+%Y-%m-%dT%H:%M:%SZ')
expires_at=$(/usr/bin/date -u -d "$generated_at +90 days" '+%Y-%m-%dT%H:%M:%SZ')

make_repository() {
    name=$1
    seed=$2
    repository="$scratch/$name"
    /usr/bin/cp -a -- "$template" "$repository"
    authority="$scratch/$name-authority"
    /usr/bin/install -d -m 0700 -- "$authority"
    printf '%s\n' "$seed" >"$authority/signing.key"
    /usr/bin/chmod 0600 -- "$authority/signing.key"
    if ! "$trusted_tool" derive-public-key --repository "$repository" \
            --signing-key "$authority/signing.key" \
            --key-id overcrow-production-2026-01 \
            --output "$repository/keys/overcrow-production-2026-01.pub" \
            >"$authority/stdout" 2>"$authority/stderr"; then
        printf 'error: %s public-key fixture failed: %s\n' \
            "$name" "$(/usr/bin/cat "$authority/stderr")" >&2
        exit 1
    fi
    test "$(/usr/bin/cat "$authority/stdout")" = 'public-key=derived'
    test ! -s "$authority/stderr"
    printf '%s\n' "$repository"
}

build_snapshot() {
    repository=$1
    sequence=$2
    generated=$3
    expires=$4
    seed=$5
    authority="$scratch/build-$sequence-$(/usr/bin/basename -- "$repository")"
    /usr/bin/install -d -m 0700 -- "$authority"
    printf '%s\n' "$sequence" >"$authority/sequence.txt"
    printf '%s\n' "$seed" >"$authority/signing.key"
    /usr/bin/chmod 0600 -- "$authority/sequence.txt" "$authority/signing.key"
    if ! "$trusted_tool" build --repository "$repository" \
            --generated-at "$generated" --expires-at "$expires" --production \
            --sequence-file "$authority/sequence.txt" \
            --sequence-state "$authority/state.json" \
            --signing-key "$authority/signing.key" \
            --key-id overcrow-production-2026-01 \
            >"$authority/stdout" 2>"$authority/stderr"; then
        printf 'error: sequence %s fixture build failed: %s\n' \
            "$sequence" "$(/usr/bin/cat "$authority/stderr")" >&2
        exit 1
    fi
    test ! -s "$authority/stdout" && test ! -s "$authority/stderr"
    /usr/bin/find "$repository/public" -type d \
        -exec /usr/bin/chmod 0755 -- {} +
    /usr/bin/cp -a -- "$repository/public" "$repository/published"
}

clone_repository() {
    source=$1
    name=$2
    destination="$scratch/$name"
    /usr/bin/cp -a -- "$source" "$destination"
    printf '%s\n' "$destination"
}

resign_catalog() {
    repository=$1
    mutation=$2
    seed=$3
    "$node_path" -e '
const crypto = require("node:crypto");
const fs = require("node:fs");
const [path, mutation, seedHex] = process.argv.slice(1);
const envelope = JSON.parse(fs.readFileSync(path, "utf8"));
const payload = JSON.parse(Buffer.from(envelope.payload, "base64url"));
const day = 24 * 60 * 60 * 1000;
switch (mutation) {
  case "sequence-one": payload.sequence = 1; break;
  case "expired":
    payload.generatedAt = "2025-01-01T00:00:00Z";
    payload.expiresAt = "2025-04-01T00:00:00Z";
    break;
  case "wrong-lifetime":
    payload.expiresAt = new Date(Date.parse(payload.generatedAt) + 89 * day)
      .toISOString().replace(".000Z", "Z");
    break;
  case "future":
    payload.generatedAt = new Date(Date.now() + 60 * 60 * 1000)
      .toISOString().replace(/\.\d{3}Z$/, "Z");
    payload.expiresAt = new Date(Date.parse(payload.generatedAt) + 90 * day)
      .toISOString().replace(".000Z", "Z");
    break;
  default: process.exit(2);
}
const payloadBytes = Buffer.from(JSON.stringify(payload));
const prefix = Buffer.from("302e020100300506032b657004220420", "hex");
const key = crypto.createPrivateKey({
  key: Buffer.concat([prefix, Buffer.from(seedHex, "hex")]),
  format: "der",
  type: "pkcs8",
});
envelope.payload = payloadBytes.toString("base64url");
envelope.signature = crypto.sign(null, payloadBytes, key).toString("base64url");
fs.writeFileSync(path, JSON.stringify(envelope));
' "$repository/published/marketplace/v1/catalog.json" "$mutation" "$seed"
    /usr/bin/chmod 0644 -- "$repository/published/marketplace/v1/catalog.json"
}

repository_name=Valhallab/playervox-overcrow-marketplace
run_gate() {
    trusted=$1
    head=$2
    event_name=${3:-pull_request}
    base_ref=${4:-master}
    head_repository=${5:-$repository_name}
    head_ref=${6:-release/2}
    sh "$trusted/$gate_relative" "$trusted" "$head" "$trusted_tool" \
        "$event_name" "$repository_name" "$base_ref" \
        "$head_repository" "$head_ref"
}

expect_accept() {
    label=$1
    shift
    if ! run_gate "$@" >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        printf '%s\n' "error: release-snapshot gate rejected $label" >&2
        exit 1
    fi
    test "$(/usr/bin/cat "$scratch/$label.stdout")" = \
        'Trusted release snapshot verified'
    test ! -s "$scratch/$label.stderr"
}

expect_reject() {
    label=$1
    shift
    if run_gate "$@" >"$scratch/$label.stdout" 2>"$scratch/$label.stderr"; then
        printf '%s\n' "error: release-snapshot gate accepted $label" >&2
        exit 1
    fi
    test ! -s "$scratch/$label.stdout"
    test "$(/usr/bin/cat "$scratch/$label.stderr")" = \
        'error: release snapshot rejected'
}

bootstrap_base=$(make_repository bootstrap-base "$production_seed")
head_one=$(make_repository head-one "$production_seed")
build_snapshot "$head_one" 1 "$generated_at" "$expires_at" "$production_seed"
expect_accept bootstrap "$bootstrap_base" "$head_one" pull_request master \
    "$repository_name" release/1

base_one=$(clone_repository "$head_one" base-one)
head_two=$(make_repository head-two "$production_seed")
build_snapshot "$head_two" 2 "$generated_at" "$expires_at" "$production_seed"
expect_accept increment "$base_one" "$head_two"

marker="$scratch/head-driver-ran"
head_driver=$(clone_repository "$head_two" malicious-head-driver)
printf '%s\n' '#!/bin/sh' "printf ran >'$marker'" 'exit 0' \
    >"$head_driver/$gate_relative"
/usr/bin/chmod 0700 -- "$head_driver/$gate_relative"
expect_accept ignores-head-driver "$base_one" "$head_driver"
test ! -e "$marker" && test ! -L "$marker"

bad_signature=$(clone_repository "$head_two" bad-signature)
"$node_path" -e '
const fs = require("node:fs");
const path = process.argv[1];
const envelope = JSON.parse(fs.readFileSync(path, "utf8"));
envelope.signature = (envelope.signature[0] === "A" ? "B" : "A")
  + envelope.signature.slice(1);
fs.writeFileSync(path, JSON.stringify(envelope));
' "$bad_signature/published/marketplace/v1/catalog.json"
/usr/bin/chmod 0644 -- "$bad_signature/published/marketplace/v1/catalog.json"
expect_reject invalid-signature "$base_one" "$bad_signature"

mutated_tree=$(clone_repository "$head_two" mutated-tree)
printf '%s\n' mutation >>"$mutated_tree/published/index.html"
expect_reject mutated-tree "$base_one" "$mutated_tree"

substituted_key=$(make_repository substituted-key "$attacker_seed")
build_snapshot "$substituted_key" 2 "$generated_at" "$expires_at" "$attacker_seed"
expect_reject substituted-key "$base_one" "$substituted_key"

changed_key_mode=$(clone_repository "$head_two" changed-key-mode)
/usr/bin/chmod 0755 -- \
    "$changed_key_mode/keys/overcrow-production-2026-01.pub"
expect_reject changed-key-mode "$base_one" "$changed_key_mode"

expired=$(clone_repository "$head_two" expired)
resign_catalog "$expired" expired "$production_seed"
expect_reject expired "$base_one" "$expired"

wrong_lifetime=$(clone_repository "$head_two" wrong-lifetime)
resign_catalog "$wrong_lifetime" wrong-lifetime "$production_seed"
expect_reject wrong-lifetime "$base_one" "$wrong_lifetime"

future=$(clone_repository "$head_two" future)
resign_catalog "$future" future "$production_seed"
expect_reject future "$base_one" "$future"

equal_sequence=$(clone_repository "$head_two" equal-sequence)
resign_catalog "$equal_sequence" sequence-one "$production_seed"
expect_reject equal-sequence "$base_one" "$equal_sequence"

base_two=$(clone_repository "$head_two" base-two)
expect_reject lower-sequence "$base_two" "$head_one"
expect_reject bootstrap-sequence-two "$bootstrap_base" "$head_two" \
    pull_request master "$repository_name" release/2

expect_reject fork-publication "$base_one" "$head_two" pull_request master \
    creator/marketplace-fork release/2
expect_reject feature-publication "$base_one" "$head_two" pull_request master \
    "$repository_name" feature/not-a-release
expect_reject candidate-publication "$base_one" "$head_two" pull_request candidate \
    "$repository_name" release/2

# Exercise the hosted driver with the same private base/head materialization as
# GitHub Actions, not only the release-gate entry point.
ci_repository="$scratch/ci-repository"
/usr/bin/cp -a -- "$bootstrap_base" "$ci_repository"
/usr/bin/install -m 0644 -- "$repo_root/marketplace/targets.json" \
    "$ci_repository/marketplace/targets.json"
/usr/bin/rm -rf -- "$ci_repository/public"
/usr/bin/git -C "$ci_repository" init --quiet --initial-branch=master
/usr/bin/git -C "$ci_repository" config user.name 'Marketplace Tests'
/usr/bin/git -C "$ci_repository" config user.email \
    'marketplace-tests@invalid.example'
/usr/bin/git -C "$ci_repository" add --all --
/usr/bin/git -C "$ci_repository" commit --quiet -m 'trusted release-gate base'
ci_base_sha=$(/usr/bin/git -C "$ci_repository" rev-parse HEAD)

run_ci_admission() {
    label=$1
    trust_sha=$2
    review_sha=$3
    release_ref=$4
    ci_trusted="$scratch/ci-trusted-$label"
    ci_private="$scratch/ci-private-$label"
    sh "$repo_root/scripts/materialize-git-snapshot.sh" --bootstrap \
        "$ci_repository" "$trust_sha" "$ci_trusted"
    /usr/bin/install -d -m 0700 -- "$ci_private"
    sh "$ci_trusted/scripts/ci-verify.sh" \
        "$ci_repository" "$trust_sha" "$review_sha" pull_request \
        "$repository_name" master "$repository_name" "$release_ref" \
        "$ci_private" admission
}

expect_ci_accept() {
    label=$1
    shift
    if ! run_ci_admission "$label" "$@" \
            >"$scratch/$label-ci.stdout" 2>"$scratch/$label-ci.stderr"; then
        printf 'error: hosted admission rejected %s: %s\n' \
            "$label" "$(/usr/bin/cat "$scratch/$label-ci.stderr")" >&2
        exit 1
    fi
    for expected_line in 'Trusted release snapshot verified' \
            'Hosted static admission passed'; do
        /usr/bin/grep -F -x -- "$expected_line" \
            "$scratch/$label-ci.stdout" >/dev/null
    done
    test ! -s "$scratch/$label-ci.stderr"
}

/usr/bin/cp -a -- "$head_one/published" "$ci_repository/published"
/usr/bin/git -C "$ci_repository" add --force -- published
/usr/bin/git -C "$ci_repository" commit --quiet -m 'valid bootstrap snapshot'
ci_one_sha=$(/usr/bin/git -C "$ci_repository" rev-parse HEAD)
expect_ci_accept ci-bootstrap "$ci_base_sha" "$ci_one_sha" release/1

/usr/bin/rm -rf -- "$ci_repository/published"
/usr/bin/cp -a -- "$head_two/published" "$ci_repository/published"
/usr/bin/git -C "$ci_repository" add --force -- published
/usr/bin/git -C "$ci_repository" commit --quiet -m 'valid increment snapshot'
ci_two_sha=$(/usr/bin/git -C "$ci_repository" rev-parse HEAD)
expect_ci_accept ci-increment "$ci_one_sha" "$ci_two_sha" release/2

# A release commit with a corrupted signature must fail for that signature,
# after the same base-owned materialization accepted the valid increment.
/usr/bin/rm -rf -- "$ci_repository/published"
/usr/bin/cp -a -- "$bad_signature/published" "$ci_repository/published"
/usr/bin/git -C "$ci_repository" add --force -- published
/usr/bin/git -C "$ci_repository" commit --quiet -m 'invalid release snapshot'
ci_head_sha=$(/usr/bin/git -C "$ci_repository" rev-parse HEAD)
if run_ci_admission invalid-signature "$ci_one_sha" "$ci_head_sha" release/2 \
        >"$scratch/ci.stdout" 2>"$scratch/ci.stderr"; then
    printf '%s\n' 'error: hosted admission accepted an invalid release snapshot' >&2
    exit 1
fi
if ! /usr/bin/grep -F -x -- 'error: release snapshot rejected' \
        "$scratch/ci.stderr" >/dev/null; then
    printf 'error: hosted release rejection was not fixed and bounded: %s\n' \
        "$(/usr/bin/cat "$scratch/ci.stderr")" >&2
    exit 1
fi

printf '%s\n' 'Release-snapshot gate smoke tests passed'
