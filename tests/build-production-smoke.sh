#!/bin/sh
set -eu
umask 077

if test "$#" -ne 0; then
    printf '%s\n' 'usage: build-production-smoke.sh' >&2
    exit 2
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
for required in scripts/build-production.sh scripts/verify-published.sh \
        scripts/prepare-marketplace-tool.sh; do
    if test ! -f "$repo_root/$required"; then
        printf '%s\n' 'error: production publisher is unavailable' >&2
        exit 1
    fi
done

scratch=$(/usr/bin/mktemp -d /tmp/marketplace-production.XXXXXXXXXX)
cleanup() { /usr/bin/rm -rf -- "$scratch"; }
trap cleanup EXIT HUP INT TERM

base="$scratch/base"
/usr/bin/install -d -m 0700 "$base"
for path in .gitignore Cargo.toml Cargo.lock rust-toolchain.toml marketplace fixtures \
        providers widgets sdk wit examples tools web scripts tests; do
    /usr/bin/cp -R -- "$repo_root/$path" "$base/"
done
/usr/bin/git init --quiet "$base"
/usr/bin/git -C "$base" config user.name 'Marketplace Tests'
/usr/bin/git -C "$base" config user.email 'marketplace-tests@invalid.example'
/usr/bin/git -C "$base" checkout --quiet -b release/fixture
/usr/bin/install -d -m 0755 "$base/published"
printf '%s\n' prior >"$base/published/prior.txt"
/usr/bin/git -C "$base" add --all
/usr/bin/git -C "$base" commit --quiet -m 'production fixture'

tool_work="$scratch/trusted-tool"
/usr/bin/install -d -m 0700 "$tool_work"
trusted_tool=$(sh "$base/scripts/prepare-marketplace-tool.sh" "$base" "$tool_work")

make_fixture() {
    name=$1
    fixture="$scratch/$name"
    /usr/bin/git clone --quiet --no-hardlinks "$base" "$fixture"
    /usr/bin/git -C "$fixture" checkout --quiet release/fixture
    printf '%s\n' "$fixture"
}

make_secrets() {
    name=$1
    seed=${2:-2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a}
    secrets="$scratch/secrets-$name"
    /usr/bin/install -d -m 0700 "$secrets"
    printf '%s\n' 1 >"$secrets/sequence.txt"
    printf '%s\n' "$seed" >"$secrets/signing.key"
    /usr/bin/chmod 0600 "$secrets/sequence.txt" "$secrets/signing.key"
    "$trusted_tool" derive-public-key --repository "$base" \
        --signing-key "$secrets/signing.key" \
        --key-id overcrow-production-2026-01 \
        --output "$secrets/signing.pub" >/dev/null
    printf '%s\n' "$secrets"
}

snapshot_published() {
    fixture=$1
    destination=$2
    /usr/bin/cp -a -- "$fixture/published" "$destination"
}

assert_unchanged() {
    fixture=$1
    before=$2
    /usr/bin/diff --recursive --no-dereference "$before" "$fixture/published"
}

run_publisher() {
    fixture=$1
    secrets=$2
    revision=$3
    key_id=${4:-overcrow-production-2026-01}
    stdout=$5
    stderr=$6
    (
        cd "$fixture"
        PATH="$scratch/fake-path:$PATH" \
            sh scripts/build-production.sh \
                --candidate-revision "$revision" \
                --sequence-file "$secrets/sequence.txt" \
                --sequence-state "$secrets/state.json" \
                --signing-key "$secrets/signing.key" \
                --public-key "$secrets/signing.pub" \
                --key-id "$key_id"
    ) >"$stdout" 2>"$stderr"
}

assert_fixed_failure() {
    label=$1
    fixture=$2
    secrets=$3
    revision=$4
    expected=$5
    key_id=${6:-overcrow-production-2026-01}
    before="$scratch/$label-published-before"
    stdout="$scratch/$label.stdout"
    stderr="$scratch/$label.stderr"
    snapshot_published "$fixture" "$before"
    if run_publisher "$fixture" "$secrets" "$revision" "$key_id" "$stdout" "$stderr"; then
        printf '%s\n' "error: $label was accepted" >&2
        exit 1
    fi
    test ! -s "$stdout"
    actual_error=$(/usr/bin/cat "$stderr")
    if test "$actual_error" != "error: $expected"; then
        case "$actual_error" in
            'error: production candidate rejected' \
            | 'error: private publisher paths rejected' \
            | 'error: production key identity rejected' \
            | 'error: production staging failed' \
            | 'error: production signing failed' \
            | 'error: production receipt rejected' \
            | 'error: production verification failed' \
            | 'error: production static tree rejected' \
            | 'error: production sequence advance failed' \
            | 'error: production publication failed')
                printf '%s\n' "error: $label returned unexpected fixed category: $actual_error" >&2
                ;;
            *)
                diagnostic_digest=$(printf '%s' "$actual_error" | /usr/bin/sha256sum \
                    | /usr/bin/cut -d ' ' -f 1)
                diagnostic_kind=unknown
                case "$actual_error" in
                    usage:*) diagnostic_kind=usage ;;
                    *'Permission denied'*) diagnostic_kind=permission ;;
                    *'cannot open'*) diagnostic_kind=cannot-open ;;
                    *'Syntax error'*) diagnostic_kind=syntax ;;
                    *'not found'*) diagnostic_kind=not-found ;;
                    *'parameter not set'*) diagnostic_kind=unset ;;
                esac
                printf '%s\n' \
                    "error: $label returned a non-fixed diagnostic kind=$diagnostic_kind bytes=${#actual_error} sha256=$diagnostic_digest" >&2
                ;;
        esac
        exit 1
    fi
    if /usr/bin/grep -F "$secrets" "$stderr" >/dev/null \
            || /usr/bin/grep -F '2a2a2a2a2a2a2a2a' "$stderr" >/dev/null; then
        printf '%s\n' "error: $label leaked private fixture material" >&2
        exit 1
    fi
    assert_unchanged "$fixture" "$before"
}

/usr/bin/install -d -m 0700 "$scratch/fake-path"
printf '%s\n' '#!/bin/sh' "printf '%s\\n' ran >'$scratch/ambient-cargo-ran'" 'exit 99' \
    >"$scratch/fake-path/cargo"
/usr/bin/chmod 0700 "$scratch/fake-path/cargo"

fixture=$(make_fixture dirty)
secrets=$(make_secrets dirty)
printf '%s\n' dirty >>"$fixture/marketplace/targets.json"
assert_fixed_failure dirty "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" \
    'production candidate rejected'

fixture=$(make_fixture branch)
secrets=$(make_secrets branch)
/usr/bin/git -C "$fixture" checkout --quiet -b candidate
assert_fixed_failure branch "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" \
    'production candidate rejected'

fixture=$(make_fixture revision)
secrets=$(make_secrets revision)
assert_fixed_failure revision "$fixture" "$secrets" \
    0000000000000000000000000000000000000000 \
    'production candidate rejected'

fixture=$(make_fixture relative)
secrets=$(make_secrets relative)
before="$scratch/relative-published-before"
snapshot_published "$fixture" "$before"
if (
        cd "$fixture"
        sh scripts/build-production.sh \
            --candidate-revision "$(/usr/bin/git rev-parse HEAD)" \
            --sequence-file relative-sequence \
            --sequence-state "$secrets/state.json" \
            --signing-key "$secrets/signing.key" \
            --public-key "$secrets/signing.pub" \
            --key-id overcrow-production-2026-01
    ) >"$scratch/relative.stdout" 2>"$scratch/relative.stderr"; then
    printf '%s\n' 'error: relative private path was accepted' >&2
    exit 1
fi
test "$(/usr/bin/cat "$scratch/relative.stderr")" = \
    'error: private publisher paths rejected'
assert_unchanged "$fixture" "$before"

fixture=$(make_fixture coincident)
secrets=$(make_secrets coincident)
before="$scratch/coincident-published-before"
snapshot_published "$fixture" "$before"
if (
        cd "$fixture"
        sh scripts/build-production.sh \
            --candidate-revision "$(/usr/bin/git rev-parse HEAD)" \
            --sequence-file "$secrets/sequence.txt" \
            --sequence-state "$secrets/sequence.txt" \
            --signing-key "$secrets/signing.key" \
            --public-key "$secrets/signing.pub" \
            --key-id overcrow-production-2026-01
    ) >"$scratch/coincident.stdout" 2>"$scratch/coincident.stderr"; then
    printf '%s\n' 'error: coincident private paths were accepted' >&2
    exit 1
fi
test "$(/usr/bin/cat "$scratch/coincident.stderr")" = \
    'error: private publisher paths rejected'
assert_unchanged "$fixture" "$before"

# The remaining cases run after the immutable sandbox stage and therefore prove
# that private inputs are opened only at the signing boundary.
fixture=$(make_fixture in-repository)
secrets=$(make_secrets in-repository)
printf '%s\n' 1 >"$fixture/inside.tmp"
/usr/bin/chmod 0600 "$fixture/inside.tmp"
before="$scratch/in-repository-published-before"
snapshot_published "$fixture" "$before"
if (
        cd "$fixture"
        sh scripts/build-production.sh \
            --candidate-revision "$(/usr/bin/git rev-parse HEAD)" \
            --sequence-file "$fixture/inside.tmp" \
            --sequence-state "$secrets/state.json" \
            --signing-key "$secrets/signing.key" \
            --public-key "$secrets/signing.pub" \
            --key-id overcrow-production-2026-01
    ) >"$scratch/in-repository.stdout" 2>"$scratch/in-repository.stderr"; then
    printf '%s\n' 'error: repository-local private path was accepted' >&2
    exit 1
fi
in_repository_error=$(/usr/bin/cat "$scratch/in-repository.stderr")
if test "$in_repository_error" != 'error: production signing failed'; then
    case "$in_repository_error" in
        'error: production candidate rejected' \
        | 'error: private publisher paths rejected' \
        | 'error: production key identity rejected' \
        | 'error: production staging failed' \
        | 'error: production signing failed' \
        | 'error: production receipt rejected' \
        | 'error: production verification failed' \
        | 'error: production static tree rejected' \
        | 'error: production sequence advance failed' \
        | 'error: production publication failed')
            printf '%s\n' \
                "error: in-repository returned unexpected fixed category: $in_repository_error" >&2
            ;;
        *)
            printf '%s\n' \
                'error: in-repository returned a non-fixed diagnostic' >&2
            ;;
    esac
    exit 1
fi
assert_unchanged "$fixture" "$before"

fixture=$(make_fixture symlink)
secrets=$(make_secrets symlink)
/usr/bin/mv "$secrets/signing.key" "$secrets/signing.real"
/usr/bin/ln -s signing.real "$secrets/signing.key"
assert_fixed_failure symlink "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production signing failed'

fixture=$(make_fixture group-readable)
secrets=$(make_secrets group-readable)
/usr/bin/chmod 0640 "$secrets/signing.key"
assert_fixed_failure group-readable "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production signing failed'

fixture=$(make_fixture development-seed)
secrets=$(make_secrets development-seed)
printf '%s\n' 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f \
    >"$secrets/signing.key"
assert_fixed_failure development-seed "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production signing failed'

fixture=$(make_fixture development-key-id)
secrets=$(make_secrets development-key-id)
assert_fixed_failure development-key-id "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" \
    'production key identity rejected' overcrow-development-2026

fixture=$(make_fixture bad-expiry-receipt)
secrets=$(make_secrets bad-expiry-receipt)
revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
printf '%s\n' \
    'schemaVersion=1' \
    "candidateRevision=$revision" \
    'keyId=overcrow-production-2026-01' \
    'sequence=1' \
    'generatedAt=2026-08-30T00:00:00Z' \
    'expiresAt=2026-09-28T23:59:59Z' \
    'payloadSha256=pending' >"$secrets/state.json.receipt"
/usr/bin/chmod 0600 "$secrets/state.json.receipt"
assert_fixed_failure bad-expiry-receipt "$fixture" "$secrets" "$revision" \
    'production receipt rejected'

fixture=$(make_fixture verifier)
secrets=$(make_secrets verifier)
other=$(make_secrets verifier-other \
    2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b)
/usr/bin/cp -- "$other/signing.pub" "$secrets/signing.pub"
assert_fixed_failure verifier "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production verification failed'
test -f "$secrets/state.json.receipt"

fixture=$(make_fixture static)
secrets=$(make_secrets static)
printf '%s\n' '<script>globalThis.bad = true</script>' >>"$fixture/web/landing/index.html"
/usr/bin/git -C "$fixture" add web/landing/index.html
/usr/bin/git -C "$fixture" commit --quiet -m 'unsafe static fixture'
assert_fixed_failure static "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production static tree rejected'

fixture=$(make_fixture advance)
secrets=$(make_secrets advance)
printf '%s\n' 9007199254740991 >"$secrets/sequence.txt"
assert_fixed_failure advance "$fixture" "$secrets" \
    "$(/usr/bin/git -C "$fixture" rev-parse HEAD)" 'production sequence advance failed'

fixture=$(make_fixture success)
secrets=$(make_secrets success)
revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
run_publisher "$fixture" "$secrets" "$revision" overcrow-production-2026-01 \
    "$scratch/success.stdout" "$scratch/success.stderr"
test ! -s "$scratch/success.stdout" && test ! -s "$scratch/success.stderr"
test -f "$fixture/published/index.html"
test -f "$fixture/published/marketplace/index.html"
test -f "$fixture/published/marketplace/v1/catalog.json"
test ! -e "$fixture/published/marketplace/policies"
/usr/bin/grep -F overcrow-production-2026-01 \
    "$fixture/published/marketplace/catalog-policy.js" >/dev/null
test "$(/usr/bin/cat "$secrets/sequence.txt")" = 2
test ! -e "$secrets/state.json.receipt" && test ! -L "$secrets/state.json.receipt"
test ! -e "$scratch/ambient-cargo-ran" && test ! -L "$scratch/ambient-cargo-ran"

fixture=$(make_fixture publication-move)
secrets=$(make_secrets publication-move)
revision=$(/usr/bin/git -C "$fixture" rev-parse HEAD)
before="$scratch/publication-move-published-before"
snapshot_published "$fixture" "$before"
(
    cd "$fixture"
    PATH="$scratch/fake-path:$PATH" \
        exec sh scripts/build-production.sh \
            --candidate-revision "$revision" \
            --sequence-file "$secrets/sequence.txt" \
            --sequence-state "$secrets/state.json" \
            --signing-key "$secrets/signing.key" \
            --public-key "$secrets/signing.pub" \
            --key-id overcrow-production-2026-01
) >"$scratch/publication-move.stdout" 2>"$scratch/publication-move.stderr" &
publisher_pid=$!
/usr/bin/install -d -m 0700 "$fixture/.published-next.$publisher_pid"
set +e
wait "$publisher_pid"
publication_status=$?
set -e
if test "$publication_status" -eq 0; then
    printf '%s\n' 'error: publication move failure was accepted' >&2
    exit 1
fi
test "$(/usr/bin/cat "$scratch/publication-move.stderr")" = \
    'error: production publication failed'
assert_unchanged "$fixture" "$before"
test "$(/usr/bin/cat "$secrets/sequence.txt")" = 2
test -f "$secrets/state.json.receipt"
/usr/bin/rm -rf -- "$fixture/.published-next.$publisher_pid"
run_publisher "$fixture" "$secrets" "$revision" overcrow-production-2026-01 \
    "$scratch/publication-retry.stdout" "$scratch/publication-retry.stderr"
test ! -s "$scratch/publication-retry.stdout" \
    && test ! -s "$scratch/publication-retry.stderr"
test "$(/usr/bin/cat "$secrets/sequence.txt")" = 3
test ! -e "$secrets/state.json.receipt" && test ! -L "$secrets/state.json.receipt"
catalog_sequence=$(node -e '
  const fs = require("node:fs");
  const envelope = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  const payload = JSON.parse(Buffer.from(envelope.payload, "base64url"));
  process.stdout.write(String(payload.sequence));
' "$fixture/published/marketplace/v1/catalog.json")
test "$catalog_sequence" = 2

printf '%s\n' 'Production publisher smoke tests passed'
