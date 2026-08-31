#!/bin/sh
set -eu
umask 077

if test "$#" -ne 12 \
        || test "$1" != --candidate-revision \
        || test "$3" != --sequence-file \
        || test "$5" != --sequence-state \
        || test "$7" != --signing-key \
        || test "$9" != --public-key \
        || test "${11}" != --key-id; then
    printf '%s\n' \
        'usage: build-production.sh --candidate-revision REVISION --sequence-file ABSOLUTE --sequence-state ABSOLUTE --signing-key ABSOLUTE --public-key ABSOLUTE --key-id overcrow-production-2026-01' >&2
    exit 2
fi
candidate_revision=$2
sequence_file=$4
sequence_state=$6
signing_key=$8
public_key=${10}
key_id=${12}

repo_root=$(pwd -P)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
if test "$script_dir" != "$repo_root/scripts" || test -L "$repo_root" \
        || test ! -f "$repo_root/Cargo.toml"; then
    printf '%s\n' 'error: production candidate rejected' >&2
    exit 1
fi
case "$candidate_revision" in
    *[!0-9a-f]* | '')
        printf '%s\n' 'error: production candidate rejected' >&2
        exit 1
        ;;
esac
if test "${#candidate_revision}" -ne 40; then
    printf '%s\n' 'error: production candidate rejected' >&2
    exit 1
fi
if test "$key_id" != overcrow-production-2026-01; then
    printf '%s\n' 'error: production key identity rejected' >&2
    exit 1
fi

valid_absolute_syntax() {
    case "$1" in
        / | '' | /*//* | */./* | */../* | */. | */.. | */) return 1 ;;
        /*) return 0 ;;
        *) return 1 ;;
    esac
}
for private_path in "$sequence_file" "$sequence_state" "$signing_key"; do
    if ! valid_absolute_syntax "$private_path"; then
        printf '%s\n' 'error: private publisher paths rejected' >&2
        exit 1
    fi
done
if test "$sequence_file" = "$sequence_state" \
        || test "$sequence_file" = "$signing_key" \
        || test "$sequence_state" = "$signing_key"; then
    printf '%s\n' 'error: private publisher paths rejected' >&2
    exit 1
fi
if test "$public_key" != "$repo_root/keys/overcrow-production-2026-01.pub"; then
    printf '%s\n' 'error: production key identity rejected' >&2
    exit 1
fi

exec 9<"$repo_root"
if ! /usr/bin/flock -n 9; then
    printf '%s\n' 'error: production candidate rejected' >&2
    exit 1
fi

trusted_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/git --no-replace-objects -C "$repo_root" "$@"
}
branch=$(trusted_git symbolic-ref --quiet --short HEAD 2>/dev/null || :)
head=$(trusted_git rev-parse --verify 'HEAD^{commit}' 2>/dev/null || :)
dirty=$(/usr/bin/mktemp /tmp/marketplace-production-status.XXXXXXXXXX)
if ! /usr/bin/timeout --signal=KILL 10 \
        /usr/bin/prlimit --cpu=5 --as=536870912 --nofile=64 --fsize=1048576 -- \
        /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
            GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
            GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
            /usr/bin/git --no-replace-objects -C "$repo_root" \
                status --porcelain=v1 --untracked-files=normal >"$dirty" 2>/dev/null; then
    /usr/bin/rm -f -- "$dirty"
    printf '%s\n' 'error: production candidate rejected' >&2
    exit 1
fi
case "$branch" in release/*) ;; *) branch='' ;; esac
if test -z "$branch" || test "$head" != "$candidate_revision" || test -s "$dirty"; then
    /usr/bin/rm -f -- "$dirty"
    printf '%s\n' 'error: production candidate rejected' >&2
    exit 1
fi
/usr/bin/rm -f -- "$dirty"

stage=$(/usr/bin/mktemp -d "$repo_root/.build-production.XXXXXXXXXX") || exit 1
source_root="$stage/repository"
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$stage"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

if ! sh "$script_dir/stage-catalog-repository.sh" \
        --mode production "$source_root" >/dev/null 2>&1; then
    printf '%s\n' 'error: production staging failed' >&2
    exit 1
fi

tool_work="$stage/publisher-tool"
/usr/bin/install -d -m 0700 "$tool_work"
trusted_tool=$(sh "$source_root/scripts/prepare-marketplace-tool.sh" \
    "$source_root" "$tool_work" 2>/dev/null) || {
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
}

# Assemble only reviewed staged bytes before any private publisher file is
# opened. The signer then adds immutable objects and catalog.json below v1.
/usr/bin/rm -rf -- "$source_root/public"
/usr/bin/install -d -m 0755 "$source_root/public"
/usr/bin/cp -R -- "$source_root/web/landing/." "$source_root/public/"
/usr/bin/install -d -m 0755 "$source_root/public/marketplace"
for file in index.html app.js styles.css; do
    /usr/bin/install -m 0644 -- "$source_root/web/marketplace/$file" \
        "$source_root/public/marketplace/$file"
done
/usr/bin/install -m 0644 -- "$source_root/web/marketplace/policies/production.js" \
    "$source_root/public/marketplace/catalog-policy.js"
/usr/bin/find "$source_root/public" -type d -exec /usr/bin/chmod 0755 {} +
/usr/bin/find "$source_root/public" -type f -exec /usr/bin/chmod 0644 {} +

# Path containment is checked only after immutable staging has completed. This
# does not dereference or open any private publisher argument.
for private_path in "$sequence_file" "$sequence_state" "$signing_key"; do
    case "$private_path" in
        "$repo_root" | "$repo_root"/*)
            printf '%s\n' 'error: production signing failed' >&2
            exit 1
            ;;
    esac
done

staged_public_key="$source_root/keys/overcrow-production-2026-01.pub"
if test ! -f "$public_key" || test -L "$public_key" \
        || test "$(/usr/bin/stat -c '%u' "$public_key" 2>/dev/null || :)" \
            != "$(/usr/bin/id -u)" \
        || test "$(/usr/bin/stat -c '%a:%h:%s' "$public_key" 2>/dev/null || :)" \
            != 644:1:65 \
        || test ! -f "$staged_public_key" || test -L "$staged_public_key" \
        || test "$(/usr/bin/stat -c '%u:%a:%h:%s' "$staged_public_key" 2>/dev/null || :)" \
            != "$(/usr/bin/id -u):600:1:65" \
        || ! /usr/bin/cmp -- "$public_key" "$staged_public_key"; then
    printf '%s\n' 'error: production key identity rejected' >&2
    exit 1
fi
public_key_fingerprint=$(
    /usr/bin/sha256sum "$staged_public_key" 2>/dev/null | /usr/bin/cut -d ' ' -f 1
) || {
    printf '%s\n' 'error: production key identity rejected' >&2
    exit 1
}
case "$public_key_fingerprint" in
    *[!0-9a-f]* | '')
        printf '%s\n' 'error: production key identity rejected' >&2
        exit 1
        ;;
esac
if test "${#public_key_fingerprint}" -ne 64 \
        || ! "$trusted_tool" verify-signing-key \
            --repository "$source_root" --signing-key "$signing_key" \
            --key-id "$key_id" >/dev/null 2>&1; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi

private_parent=${sequence_state%/*}
receipt="$sequence_state.receipt"
if test "$private_parent" = "$sequence_state" || test "$receipt" = "$sequence_file" \
        || test "$receipt" = "$signing_key" \
        || test ! -d "$private_parent" || test -L "$private_parent" \
        || test "$(CDPATH='' cd -- "$private_parent" 2>/dev/null && pwd -P || :)" \
            != "$private_parent" \
        || test "$(/usr/bin/stat -c '%u:%a' "$private_parent" 2>/dev/null || :)" \
            != "$(/usr/bin/id -u):700"; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi

copy_private_file() {
    source=$1
    destination=$2
    maximum=$3
    test -f "$source" && test ! -L "$source" \
        && test "$(/usr/bin/stat -c '%u:%a:%h' "$source" 2>/dev/null || :)" \
            = "$(/usr/bin/id -u):600:1" \
        && test "$(/usr/bin/stat -c '%s' "$source" 2>/dev/null || :)" \
            -le "$maximum" || return 1
    /usr/bin/timeout --signal=KILL 2 /usr/bin/dd if="$source" of="$destination" \
        iflag=nofollow,nonblock bs="$maximum" count=1 status=none 2>/dev/null \
        || return 1
    test "$(/usr/bin/stat -c '%s' "$destination" 2>/dev/null || :)" -le "$maximum" \
        && test "$(/usr/bin/stat -c '%u:%a:%h' "$source" 2>/dev/null || :)" \
            = "$(/usr/bin/id -u):600:1"
}

counter_copy="$stage/sequence.txt"
if ! copy_private_file "$sequence_file" "$counter_copy" 32; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi
sequence=$(/usr/bin/cat "$counter_copy")
case "$sequence" in 0 | 0* | *[!0-9]* | '')
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
    ;;
esac
test "${#sequence}" -le 20 || {
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
}

write_receipt() {
    payload_digest=$1
    {
        temporary=$(/usr/bin/mktemp "$private_parent/.production-receipt.XXXXXXXXXX") \
            || return 1
        if ! /usr/bin/chmod 0600 "$temporary" \
                || ! printf '%s\n' \
                    'schemaVersion=1' \
                    "candidateRevision=$candidate_revision" \
                    "keyId=$key_id" \
                    "publicKeySha256=$public_key_fingerprint" \
                    "sequence=$sequence" \
                    "generatedAt=$generated_at" \
                    "expiresAt=$expires_at" \
                    "payloadSha256=$payload_digest" >"$temporary" \
                || ! /usr/bin/sync -f "$temporary" \
                || ! /usr/bin/mv -T -- "$temporary" "$receipt"; then
            /usr/bin/rm -f -- "$temporary" || :
            return 1
        fi
        /usr/bin/sync -f "$private_parent"
    } >/dev/null 2>&1
}

receipt_copy="$stage/receipt"
if test -e "$receipt" || test -L "$receipt"; then
    if ! copy_private_file "$receipt" "$receipt_copy" 1024 \
            || test "$(/usr/bin/wc -l <"$receipt_copy")" -ne 8 \
            || test "$(/usr/bin/sed -n '1p' "$receipt_copy")" != schemaVersion=1; then
        printf '%s\n' 'error: production receipt rejected' >&2
        exit 1
    fi
    receipt_candidate=$(/usr/bin/sed -n 's/^candidateRevision=//p' "$receipt_copy")
    receipt_key=$(/usr/bin/sed -n 's/^keyId=//p' "$receipt_copy")
    receipt_public_key=$(/usr/bin/sed -n 's/^publicKeySha256=//p' "$receipt_copy")
    receipt_sequence=$(/usr/bin/sed -n 's/^sequence=//p' "$receipt_copy")
    case "$receipt_sequence:$receipt_public_key" in
        *[!0-9:abcdef]* | 0:* | 0*:* | :*)
            printf '%s\n' 'error: production receipt rejected' >&2
            exit 1
            ;;
    esac
    if test "${#receipt_sequence}" -gt 16 \
            || test "${#receipt_public_key}" -ne 64; then
        printf '%s\n' 'error: production receipt rejected' >&2
        exit 1
    fi
    if test "$receipt_sequence" -eq "$sequence"; then
        if test "$receipt_candidate" != "$candidate_revision" \
                || test "$receipt_key" != "$key_id" \
                || test "$receipt_public_key" != "$public_key_fingerprint"; then
            printf '%s\n' 'error: production receipt rejected' >&2
            exit 1
        fi
        generated_at=$(/usr/bin/sed -n 's/^generatedAt=//p' "$receipt_copy")
        expires_at=$(/usr/bin/sed -n 's/^expiresAt=//p' "$receipt_copy")
        receipt_payload=$(/usr/bin/sed -n 's/^payloadSha256=//p' "$receipt_copy")
        expected_expires=$(/usr/bin/date -u -d "$generated_at +30 days" '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || :)
        case "$receipt_payload" in
            pending) ;;
            *[!0-9a-f]* | '') receipt_payload='' ;;
        esac
        if test "${#generated_at}" -ne 20 || test "${#expires_at}" -ne 20 \
                || test "$expires_at" != "$expected_expires" \
                || { test "$receipt_payload" != pending \
                    && test "${#receipt_payload}" -ne 64; }; then
            printf '%s\n' 'error: production receipt rejected' >&2
            exit 1
        fi
    elif test "$receipt_sequence" -lt "$sequence"; then
        generated_at=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%SZ')
        expires_at=$(/usr/bin/date -u -d "$generated_at +30 days" '+%Y-%m-%dT%H:%M:%SZ')
        receipt_payload=pending
        write_receipt "$receipt_payload" || {
            printf '%s\n' 'error: production receipt rejected' >&2
            exit 1
        }
    else
        printf '%s\n' 'error: production receipt rejected' >&2
        exit 1
    fi
else
    generated_at=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%SZ')
    expires_at=$(/usr/bin/date -u -d "$generated_at +30 days" '+%Y-%m-%dT%H:%M:%SZ')
    receipt_payload=pending
    write_receipt "$receipt_payload" || {
        printf '%s\n' 'error: production receipt rejected' >&2
        exit 1
    }
fi

if ! "$trusted_tool" build \
        --repository "$source_root" \
        --generated-at "$generated_at" \
        --expires-at "$expires_at" \
        --production \
        --sequence-file "$sequence_file" \
        --sequence-state "$sequence_state" \
        --signing-key "$signing_key" \
        --key-id "$key_id" >/dev/null 2>&1; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi
# Generated object directories are created under this script's private umask.
# Normalize the complete public result to its reviewed serving modes before it
# can be verified, ledgered, or accepted by the sequence authority.
if ! /usr/bin/find "$source_root/public" -type d -exec /usr/bin/chmod 0755 {} + \
        || ! /usr/bin/find "$source_root/public" -type f -exec /usr/bin/chmod 0644 {} +; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi

state_copy="$stage/state.json"
if ! copy_private_file "$sequence_state" "$state_copy" 512; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi
state_sequence=$(/usr/bin/sed -n 's/.*"sequence":\([0-9][0-9]*\).*/\1/p' "$state_copy")
payload_digest=$(/usr/bin/sed -n 's/.*"payloadSha256":"\([0-9a-f][0-9a-f]*\)".*/\1/p' "$state_copy")
if test "$state_sequence" != "$sequence" || test "${#payload_digest}" -ne 64; then
    printf '%s\n' 'error: production signing failed' >&2
    exit 1
fi
if test "$receipt_payload" != pending && test "$receipt_payload" != "$payload_digest"; then
    printf '%s\n' 'error: production receipt rejected' >&2
    exit 1
fi
if test "$receipt_payload" = pending; then
    write_receipt "$payload_digest" || {
        printf '%s\n' 'error: production receipt rejected' >&2
        exit 1
    }
fi

catalog="$source_root/public/marketplace/v1/catalog.json"
if ! "$trusted_tool" verify "$catalog" --public-key "$staged_public_key" \
        --key-id "$key_id" >/dev/null 2>&1; then
    printf '%s\n' 'error: production verification failed' >&2
    exit 1
fi
if ! sh "$script_dir/verify-published.sh" "$source_root/public" \
        "$staged_public_key" "$key_id" >/dev/null 2>&1; then
    printf '%s\n' 'error: production static tree rejected' >&2
    exit 1
fi
tree_ledger="$stage/final-tree.ledger"
if ! "$trusted_tool" write-tree-ledger --repository "$source_root" \
        --tree "$source_root/public" --output "$tree_ledger" >/dev/null 2>&1; then
    printf '%s\n' 'error: production static tree rejected' >&2
    exit 1
fi
tree_ledger_sha256=$(
    /usr/bin/sha256sum "$tree_ledger" 2>/dev/null | /usr/bin/cut -d ' ' -f 1
) || {
    printf '%s\n' 'error: production static tree rejected' >&2
    exit 1
}
case "$tree_ledger_sha256" in
    *[!0-9a-f]* | '') tree_ledger_sha256='' ;;
esac
if test "${#tree_ledger_sha256}" -ne 64; then
    printf '%s\n' 'error: production static tree rejected' >&2
    exit 1
fi
if ! "$trusted_tool" advance-sequence \
        --repository "$source_root" \
        --sequence-file "$sequence_file" \
        --sequence-state "$sequence_state" \
        --catalog "$catalog" >/dev/null 2>&1; then
    printf '%s\n' 'error: production sequence advance failed' >&2
    exit 1
fi
if test ! -f "$receipt" || test -L "$receipt" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$receipt" 2>/dev/null || :)" \
            != "$(/usr/bin/id -u):600:1"; then
    printf '%s\n' 'error: production receipt rejected' >&2
    exit 1
fi

next_published="$repo_root/.published-next.$$"
previous_published="$repo_root/.published-previous.$$"
if ! sh "$script_dir/publish-directory.sh" \
        "$source_root/public" "$repo_root/published" \
        "$next_published" "$previous_published" \
        /usr/bin/mv /usr/bin/mv /usr/bin/mv /usr/bin/mv \
        "$trusted_tool" "$tree_ledger" "$tree_ledger_sha256" >/dev/null 2>&1; then
    printf '%s\n' 'error: production publication failed' >&2
    exit 1
fi
/usr/bin/rm -f -- "$receipt" >/dev/null 2>&1 || :
/usr/bin/sync -f "$private_parent" >/dev/null 2>&1 || :
