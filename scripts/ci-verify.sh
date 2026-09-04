#!/bin/sh
set -eu
umask 077

usage() {
    printf '%s\n' \
        'usage: ci-verify.sh [REPOSITORY TRUST-SHA REVIEW-SHA EVENT REPOSITORY-NAME BASE-REF HEAD-REPOSITORY HEAD-REF PRIVATE-PARENT admission [ACCEPTED-STORE]]' >&2
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
trusted_root=$(/usr/bin/dirname -- "$script_dir")

run_local_checks() {
    local_root=$1
    (
        CDPATH='' cd -- "$local_root"
        cargo test -p marketplace-tool --locked
        node --test tests/warframe-market/market.test.mjs
        node --test tests/site-runtime.test.js
        local_tmp=$(mktemp -d)
        trap 'rm -rf "$local_tmp"' EXIT HUP INT TERM
        cargo run -p marketplace-tool --locked --quiet -- \
            package fixtures/hello-web "$local_tmp/hello.ocpkg"
        cargo run -p marketplace-tool --locked --quiet -- \
            inspect "$local_tmp/hello.ocpkg" >/dev/null
        cargo run -p marketplace-tool --locked --quiet -- \
            package widgets/warframe-market \
            "$local_tmp/warframe-market.ocpkg"
        cargo run -p marketplace-tool --locked --quiet -- \
            inspect "$local_tmp/warframe-market.ocpkg" >/dev/null
    )
}

if test "$#" -eq 0; then
    run_local_checks "$trusted_root"
    exit 0
fi
if { test "$#" -ne 10 && test "$#" -ne 11; } \
        || test "${10}" != admission; then
    usage
    exit 2
fi

repository=$1
trust_sha=$2
review_sha=$3
event_name=$4
repository_name=$5
base_ref=$6
head_repository=$7
head_ref=$8
private_parent=$9
accepted_store=${11:-}

valid_revision() {
    case "$1" in '' | *[!0-9a-f]*) return 1 ;; esac
    test "${#1}" -eq 40
}

safe_owned_directory() {
    directory=$1
    expected_mode=${2:-}
    test -d "$directory" && test ! -L "$directory" \
        && test "$(CDPATH='' cd -- "$directory" 2>/dev/null && pwd -P || :)" \
            = "$directory" \
        && test "$(/usr/bin/stat -c '%u' "$directory" 2>/dev/null || :)" \
            = "$(/usr/bin/id -u)" \
        && test -z "$(/usr/bin/find "$directory" -maxdepth 0 \
            -perm /0022 -print -quit)" \
        && { test -z "$expected_mode" \
            || test "$(/usr/bin/stat -c '%a' "$directory")" \
                = "$expected_mode"; }
}

case "$event_name" in pull_request | push) ;; *)
    fail 'CI trust metadata is invalid'
    ;;
esac
case "$base_ref" in master | candidate) ;; *)
    fail 'CI trust metadata is invalid'
    ;;
esac
if ! valid_revision "$trust_sha" || ! valid_revision "$review_sha"; then
    fail 'CI trust metadata is invalid'
fi
case "$repository:$private_parent" in /*:/*) ;; *)
    fail 'CI trust metadata is invalid'
    ;;
esac
if test "$repository" = / || test "$trusted_root" = / \
        || test "$private_parent" = / \
        || ! safe_owned_directory "$repository" \
        || ! safe_owned_directory "$trusted_root" \
        || ! safe_owned_directory "$private_parent" 700; then
    fail 'CI trust roots are unsafe'
fi
if test -n "$accepted_store"; then
    case "$accepted_store" in /*) ;; *)
        fail 'accepted artifact store is unsafe'
        ;;
    esac
    case "$accepted_store" in
        "$repository" | "$repository"/* | "$trusted_root" | "$trusted_root"/*)
            fail 'accepted artifact store is unsafe'
            ;;
    esac
    if test "$event_name" != push \
            || ! safe_owned_directory "$accepted_store" 700; then
        fail 'accepted artifact store is unsafe'
    fi
fi
if test "$event_name" = push \
        && { test "$trust_sha" != "$review_sha" \
            || test "$repository_name" != "$head_repository"; }; then
    fail 'CI trust metadata is invalid'
fi
for required in Cargo.lock Cargo.toml rust-toolchain.toml \
        scripts/ci-verify.sh scripts/materialize-git-snapshot.sh \
        scripts/resolve-pinned-rust.sh \
        scripts/resolve-system-node.sh \
        tests/reject-published-change.sh tests/reject-trusted-change.sh \
        tools/marketplace-tool/Cargo.toml \
        tools/marketplace-tool/src/admission.rs \
        tools/marketplace-tool/src/main.rs \
        tools/marketplace-tool/src/package.rs \
        tools/marketplace-tool/src/snapshot.rs; do
    if test ! -f "$trusted_root/$required" \
            || test -L "$trusted_root/$required"; then
        fail 'trusted CI driver is incomplete'
    fi
done

ci_git() {
    /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C \
        GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
        GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0 \
        /usr/bin/timeout --signal=KILL 15 \
        /usr/bin/prlimit --cpu=10 --as=1073741824 --nofile=128 \
            --fsize=33554432 -- \
        /usr/bin/git --no-replace-objects \
            -c core.fsmonitor=false -c core.hooksPath=/dev/null \
            -c core.attributesFile=/dev/null -c core.excludesFile=/dev/null \
            -c commit.gpgSign=false -c diff.external= -C "$repository" "$@"
}

resolved_trust=$(ci_git rev-parse --verify "$trust_sha^{commit}" 2>/dev/null) \
    || resolved_trust=''
resolved_review=$(ci_git rev-parse --verify "$review_sha^{commit}" 2>/dev/null) \
    || resolved_review=''
review_tree=$(ci_git rev-parse --verify "$review_sha^{tree}" 2>/dev/null) \
    || review_tree=''
if test "$resolved_trust" != "$trust_sha" \
        || test "$resolved_review" != "$review_sha" \
        || ! valid_revision "$review_tree"; then
    fail 'CI trust metadata is invalid'
fi

work=$(/usr/bin/mktemp -d "$private_parent/verification.XXXXXXXXXX") \
    || exit 1
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    /usr/bin/rm -rf -- "$work"
    exit "$status"
}
trap cleanup EXIT HUP INT TERM

changed_paths="$work/changed-paths.nul"
/usr/bin/install -m 0600 /dev/null "$changed_paths"
if test "$event_name" = pull_request; then
    if ! ci_git diff --name-only -z --no-renames \
            "$trust_sha" "$review_sha" -- >"$changed_paths" \
            || test "$(/usr/bin/stat -c '%u:%a:%h' "$changed_paths")" \
                != "$(/usr/bin/id -u):600:1" \
            || test "$(/usr/bin/stat -c '%s' "$changed_paths")" \
                -gt 262144; then
        fail 'pull-request path metadata is unsafe'
    fi
    sh "$trusted_root/tests/reject-published-change.sh" \
        pull_request "$repository_name" "$base_ref" \
        "$head_repository" "$head_ref"
    /usr/bin/xargs -0 -r -- \
        sh "$trusted_root/tests/reject-published-change.sh" \
            pull_request "$repository_name" "$base_ref" \
            "$head_repository" "$head_ref" <"$changed_paths"
    /usr/bin/xargs -0 -r -- \
        sh "$trusted_root/tests/reject-trusted-change.sh" <"$changed_paths"
fi

resolved_rust=$(sh "$trusted_root/scripts/resolve-pinned-rust.sh" \
    "$trusted_root") || {
    fail 'trusted marketplace tool is unavailable'
}
tab=$(printf '\t')
IFS="$tab" read -r toolchain_root cargo_path rustc_path \
    cargo_index cargo_cache cargo_sources <<EOF
$resolved_rust
EOF
if test -z "$cargo_sources"; then
    fail 'trusted marketplace tool is unavailable'
fi
tool_home="$work/home"
tool_cargo_home="$work/cargo-home"
tool_rustup_home="$work/rustup-home"
tool_target="$work/target"
if ! /usr/bin/install -d -m 0700 \
        "$tool_home" "$tool_cargo_home/registry" \
        "$tool_rustup_home" "$tool_target" \
        || ! /usr/bin/ln -s -- "$cargo_index" \
            "$tool_cargo_home/registry/index" \
        || ! /usr/bin/ln -s -- "$cargo_cache" \
            "$tool_cargo_home/registry/cache" \
        || ! /usr/bin/ln -s -- "$cargo_sources" \
            "$tool_cargo_home/registry/src"; then
    fail 'trusted marketplace tool is unavailable'
fi
trusted_cargo() {
    (CDPATH='' cd / && \
        /usr/bin/env -i \
            PATH="$toolchain_root/bin:/usr/bin:/bin" \
            HOME="$tool_home" CARGO_HOME="$tool_cargo_home" \
            RUSTUP_HOME="$tool_rustup_home" RUSTC="$rustc_path" \
            CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
            CARGO_TARGET_DIR="$tool_target" LC_ALL=C.UTF-8 LANG=C.UTF-8 \
            /usr/bin/timeout --signal=TERM --kill-after=5 180 \
            /usr/bin/prlimit --cpu=120 --as=4294967296 --nproc=4096 \
                --nofile=256 --fsize=268435456 -- \
            "$cargo_path" "$@")
}
if test "$event_name" = push; then
    node_path=$(sh "$trusted_root/scripts/resolve-system-node.sh") || {
        fail 'trusted source checks failed'
    }
    if ! trusted_cargo test --manifest-path \
            "$trusted_root/tools/marketplace-tool/Cargo.toml" \
            --package marketplace-tool --locked --offline --quiet \
            || ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 LANG=C.UTF-8 \
                /usr/bin/timeout --signal=TERM --kill-after=5 60 \
                "$node_path" --test \
                    "$trusted_root/tests/warframe-market/market.test.mjs" \
            || ! /usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 LANG=C.UTF-8 \
                /usr/bin/timeout --signal=TERM --kill-after=5 60 \
                "$node_path" --test "$trusted_root/tests/site-runtime.test.js"; then
        fail 'trusted source checks failed'
    fi
fi
if ! trusted_cargo build --manifest-path \
        "$trusted_root/tools/marketplace-tool/Cargo.toml" \
        --package marketplace-tool --locked --offline --quiet; then
    fail 'trusted marketplace tool is unavailable'
fi
built_tool="$tool_target/debug/marketplace-tool"
trusted_tool="$work/marketplace-tool"
if test ! -f "$built_tool" || test -L "$built_tool" \
        || ! /usr/bin/install -m 0700 -- "$built_tool" "$trusted_tool" \
        || test "$(/usr/bin/stat -c '%u:%a:%h' "$trusted_tool")" \
            != "$(/usr/bin/id -u):700:1"; then
    fail 'trusted marketplace tool is unavailable'
fi

candidate_root="$work/candidate"
if ! sh "$trusted_root/scripts/materialize-git-snapshot.sh" --validated \
        "$repository" "$review_sha" "$candidate_root" "$trusted_tool"; then
    fail 'candidate snapshot admission failed'
fi
widgets_root="$candidate_root/widgets"
widget_list="$work/widgets"
if test ! -d "$widgets_root" || test -L "$widgets_root" \
        || test -n "$(/usr/bin/find "$widgets_root" -mindepth 1 -maxdepth 1 \
            ! -type d -print -quit)" \
        || ! /usr/bin/find "$widgets_root" -mindepth 1 -maxdepth 1 \
            -type d -printf '%f\n' | LC_ALL=C /usr/bin/sort >"$widget_list";
then
    fail 'candidate artifact admission failed'
fi
widget_count=$(/usr/bin/wc -l <"$widget_list")
if test "$widget_count" -eq 0 || test "$widget_count" -gt 512 \
        || test "$(/usr/bin/stat -c '%s' "$widget_list")" -gt 131072; then
    fail 'candidate artifact admission failed'
fi

artifact_root="$work/artifacts"
receipt="$work/admission-receipt.tsv"
identities="$work/identities"
/usr/bin/install -d -m 0700 -- "$artifact_root"
/usr/bin/install -m 0600 /dev/null "$receipt"
/usr/bin/install -m 0600 /dev/null "$identities"
printf 'admission\t1\t%s\t%s\t%s\n' \
    "$trust_sha" "$review_sha" "$review_tree" >"$receipt"
artifact_index=0
while IFS= read -r directory; do
    case "$directory" in
        '' | -* | *[!A-Za-z0-9._-]*)
            fail 'candidate artifact admission failed'
            ;;
    esac
    artifact_index=$((artifact_index + 1))
    source="$widgets_root/$directory"
    artifact="$artifact_root/$artifact_index.ocpkg"
    package_output="$work/package-$artifact_index.out"
    inspect_output="$work/inspect-$artifact_index.out"
    if ! "$trusted_tool" package "$source" "$artifact" \
            >"$package_output" 2>/dev/null \
            || ! "$trusted_tool" inspect "$artifact" \
                >"$inspect_output" 2>/dev/null; then
        fail 'candidate artifact admission failed'
    fi
    package_digest=$(/usr/bin/awk 'NR == 1 { print $1 }' "$package_output")
    package_path=$(/usr/bin/cut -d ' ' -f 2- "$package_output")
    digest=$(/usr/bin/sha256sum "$artifact" | /usr/bin/cut -d ' ' -f 1)
    bytes=$(/usr/bin/stat -c '%s' "$artifact")
    IFS=' ' read -r extension_id extension_version extra <<EOF
$(/usr/bin/cat "$inspect_output")
EOF
    if test -n "${extra:-}" || test -z "${extension_id:-}" \
            || test -z "${extension_version:-}" \
            || test "$package_digest" != "$digest" \
            || test "$package_path" != "$artifact"; then
        fail 'candidate artifact admission failed'
    fi
    printf '%s\n' "$extension_id" >>"$identities"
    printf 'artifact\twidgets/%s\t%s\t%s\t%s\t%s\n' \
        "$directory" "$extension_id" "$extension_version" \
        "$digest" "$bytes" >>"$receipt"
done <"$widget_list"
if test "$artifact_index" -ne "$widget_count" \
        || test -n "$(LC_ALL=C /usr/bin/sort "$identities" \
            | /usr/bin/uniq -d)"; then
    fail 'candidate artifact admission failed'
fi

if test -n "$accepted_store" \
        && ! "$trusted_tool" ingest \
            --receipt "$receipt" --artifacts "$artifact_root" \
            --store "$accepted_store" --trust-sha "$trust_sha" \
            --review-sha "$review_sha" --review-tree "$review_tree" \
            >/dev/null; then
    fail 'accepted artifact ingestion failed'
fi

/usr/bin/cat "$receipt"
printf '%s\n' 'Hosted static admission passed'
