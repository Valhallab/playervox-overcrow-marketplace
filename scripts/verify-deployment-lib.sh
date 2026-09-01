# shellcheck shell=sh
# Source-only implementation for verify-deployment.sh and its loopback smoke.

if ! (return 0 2>/dev/null); then
    printf '%s\n' 'error: deployment verifier library must be sourced' >&2
    return 1
fi

case "${MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT-}" in
    production-wrapper | fixture-runner)
        verify_deployment_source_context=$MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT
        ;;
    *)
        printf '%s\n' 'error: deployment verifier source context is invalid' >&2
        return 1
        ;;
esac
unset MARKETPLACE_VERIFY_DEPLOYMENT_SOURCE_CONTEXT

_verify_marketplace_deployment() (
    set -eu
    umask 077

    if test "$#" -ne 6; then
        return 2
    fi
    deployment_repo_root=$1
    deployment_mode=$2
    deployment_origin=$3
    deployment_expected_tree=$4
    deployment_public_key=$5
    deployment_key_id=$6

    case "$deployment_mode:$deployment_origin" in
        production:https://overcrow.playervox.com)
            deployment_protocol=https
            ;;
        fixture-loopback:http://127.0.0.1:[0-9]*)
            deployment_protocol=http
            deployment_port=${deployment_origin##*:}
            case "$deployment_port" in '' | *[!0-9]*) return 1 ;; esac
            test "$deployment_port" -ge 1 && test "$deployment_port" -le 65535 \
                || return 1
            ;;
        *) return 1 ;;
    esac

    case "$deployment_repo_root:$deployment_expected_tree:$deployment_public_key" in
        /*:/*:/*) ;;
        *) return 1 ;;
    esac
    if test ! -d "$deployment_repo_root" || test -L "$deployment_repo_root" \
            || test "$(CDPATH='' cd -- "$deployment_repo_root" && pwd -P)" \
                != "$deployment_repo_root" \
            || test "$deployment_key_id" != overcrow-production-2026-01 \
            || test "$deployment_expected_tree" != "$deployment_repo_root/published" \
            || test "$deployment_public_key" \
                != "$deployment_repo_root/keys/overcrow-production-2026-01.pub" \
            || test ! -d "$deployment_expected_tree" \
            || test -L "$deployment_expected_tree"; then
        return 1
    fi

    deployment_work=$(/usr/bin/mktemp -d /tmp/marketplace-deployment.XXXXXXXXXX)
    # shellcheck disable=SC2329 # Invoked by the traps below.
    deployment_cleanup() {
        deployment_status=$?
        trap - EXIT HUP INT TERM
        /usr/bin/rm -rf -- "$deployment_work"
        exit "$deployment_status"
    }
    trap deployment_cleanup EXIT HUP INT TERM

    # Reject unsafe or oversized local releases before compiling the authority
    # tool or issuing the first request. The verified tree is the only source
    # of remote paths and expected bytes.
    if test -n "$(/usr/bin/find "$deployment_expected_tree" -xdev \
            ! -type d ! -type f -print -quit)"; then
        return 1
    fi
    deployment_manifest="$deployment_work/expected.tsv"
    deployment_tab=$(printf '\t')
    : >"$deployment_manifest"
    /usr/bin/find "$deployment_expected_tree" -xdev -type f -printf '%P\n' \
        | LC_ALL=C /usr/bin/sort \
        | (
            deployment_count=0
            deployment_total=0
            while IFS= read -r deployment_relative; do
                case "$deployment_relative" in
                    '' | *"$deployment_tab"* | /* | *'//'*) exit 1 ;;
                esac
                deployment_count=$((deployment_count + 1))
                test "$deployment_count" -le 1000 || exit 1
                deployment_expected="$deployment_expected_tree/$deployment_relative"
                test -f "$deployment_expected" && test ! -L "$deployment_expected" \
                    || exit 1
                deployment_size=$(/usr/bin/stat -c '%s' "$deployment_expected")
                case "$deployment_size" in '' | *[!0-9]*) exit 1 ;; esac
                test "$deployment_size" -le 16777216 || exit 1
                deployment_total=$((deployment_total + deployment_size))
                test "$deployment_total" -le 268435456 || exit 1
                if test "$deployment_relative" = marketplace/v1/catalog.json; then
                    test "$deployment_size" -le 1048576 || exit 1
                fi
                printf '%s\t%s\n' "$deployment_relative" "$deployment_size" \
                    >>"$deployment_manifest"
            done
            test "$deployment_count" -gt 0
        )

    deployment_tool_work="$deployment_work/tool"
    /usr/bin/install -d -m 0700 -- "$deployment_tool_work"
    deployment_tool=$(sh "$deployment_repo_root/scripts/prepare-marketplace-tool.sh" \
        "$deployment_repo_root" "$deployment_tool_work")
    deployment_catalog="$deployment_expected_tree/marketplace/v1/catalog.json"
    "$deployment_tool" verify "$deployment_catalog" \
        --public-key "$deployment_public_key" --key-id "$deployment_key_id" \
        >/dev/null
    "$deployment_tool" verify-tree --repository "$deployment_repo_root" \
        --tree "$deployment_expected_tree" --public-key "$deployment_public_key" \
        --key-id "$deployment_key_id" >/dev/null

    deployment_fetch() {
        deployment_fetch_relative=$1
        deployment_fetch_destination=$2
        deployment_fetch_headers=$3
        deployment_fetch_maximum=$4
        case "$deployment_fetch_relative" in /* | *'//'*) return 1 ;; esac
        case "$deployment_fetch_maximum" in '' | *[!0-9]*) return 1 ;; esac
        if test "$deployment_fetch_maximum" -eq 0; then
            deployment_fetch_maximum=1
        fi
        deployment_fetch_code=$(
            /usr/bin/env -i LC_ALL=C /usr/bin/curl --disable \
                --proto "=$deployment_protocol" --noproxy '*' \
                --fail --silent --show-error --connect-timeout 5 --max-time 30 \
                --max-redirs 0 --max-filesize "$deployment_fetch_maximum" \
                --output "$deployment_fetch_destination" \
                --dump-header "$deployment_fetch_headers" \
                --write-out '%{http_code}' \
                "$deployment_origin/$deployment_fetch_relative"
        ) || return 1
        test "$deployment_fetch_code" = 200
    }

    deployment_check_headers() {
        deployment_headers=$1
        deployment_expected_cache=$2
        deployment_expected_type=$3
        LC_ALL=C /usr/bin/awk \
            -v expected_cache="$deployment_expected_cache" \
            -v expected_type="$deployment_expected_type" '
function trim(value) {
    sub(/^[[:space:]]+/, "", value)
    sub(/[[:space:]]+$/, "", value)
    return value
}
BEGIN {
    status_count = 0
    cache_count = 0
    type_count = 0
    invalid = 0
}
{
    sub(/\r$/, "")
    if ($0 ~ /^HTTP\//) {
        status_count++
        split($0, status, /[[:space:]]+/)
        if (status[2] != "200") invalid = 1
        next
    }
    separator = index($0, ":")
    if (separator == 0) next
    name = tolower(substr($0, 1, separator - 1))
    value = tolower(trim(substr($0, separator + 1)))
    if (name == "cache-control") {
        cache_count++
        cache_value = value
    }
    if (name == "content-type") {
        type_count++
        split(value, parts, ";")
        media_type = trim(parts[1])
    }
}
END {
    if (status_count != 1 || type_count != 1 || media_type !~ expected_type) {
        invalid = 1
    }
    if (expected_cache == "optional") {
        if (cache_count > 1 || (cache_count == 1 && cache_value ~ /immutable/)) {
            invalid = 1
        }
    } else if (cache_count != 1 || cache_value != expected_cache) {
        invalid = 1
    }
    exit invalid
}' "$deployment_headers"
    }

    deployment_fetch_compare() {
        deployment_relative=$1
        deployment_route=$2
        deployment_expected=$3
        deployment_cache=$4
        deployment_type=$5
        deployment_size=$(/usr/bin/stat -c '%s' "$deployment_expected")
        deployment_destination="$deployment_work/response"
        deployment_headers="$deployment_work/response.headers"
        deployment_fetch "$deployment_route" "$deployment_destination" \
            "$deployment_headers" "$deployment_size"
        test "$(/usr/bin/stat -c '%s' "$deployment_destination")" \
            = "$deployment_size"
        /usr/bin/cmp --silent "$deployment_destination" "$deployment_expected"
        deployment_check_headers "$deployment_headers" "$deployment_cache" \
            "$deployment_type"
    }

    deployment_fetch_compare index.html '' \
        "$deployment_expected_tree/index.html" optional '^text/html$'
    deployment_fetch_compare marketplace/index.html marketplace/ \
        "$deployment_expected_tree/marketplace/index.html" optional '^text/html$'

    deployment_remote_catalog="$deployment_work/catalog.json"
    deployment_catalog_headers="$deployment_work/catalog.headers"
    deployment_catalog_size=$(/usr/bin/stat -c '%s' "$deployment_catalog")
    deployment_fetch marketplace/v1/catalog.json "$deployment_remote_catalog" \
        "$deployment_catalog_headers" "$deployment_catalog_size"
    /usr/bin/cmp --silent "$deployment_remote_catalog" "$deployment_catalog"
    "$deployment_tool" verify "$deployment_remote_catalog" \
        --public-key "$deployment_public_key" --key-id "$deployment_key_id" \
        >/dev/null
    deployment_check_headers "$deployment_catalog_headers" no-cache \
        '^application/json$'

    while IFS="$deployment_tab" read -r deployment_relative deployment_size; do
        case "$deployment_relative" in
            index.html | marketplace/index.html | marketplace/v1/catalog.json)
                continue
                ;;
        esac
        deployment_cache=optional
        case "$deployment_relative" in
            marketplace/catalog-policy.js)
                deployment_cache=no-cache
                deployment_type='^(application/javascript|text/javascript)$'
                ;;
            marketplace/v1/packages/*/*.ocpkg)
                deployment_cache='public, max-age=31536000, immutable'
                deployment_type='^application/octet-stream$'
                ;;
            marketplace/v1/previews/*/*.png)
                deployment_cache='public, max-age=31536000, immutable'
                deployment_type='^image/png$'
                ;;
            *.html) deployment_type='^text/html$' ;;
            *.js) deployment_type='^(application/javascript|text/javascript)$' ;;
            *.css) deployment_type='^text/css$' ;;
            *.png) deployment_type='^image/png$' ;;
            *.svg) deployment_type='^image/svg[+]xml$' ;;
            *.jpg | *.jpeg) deployment_type='^image/jpeg$' ;;
            *.webp) deployment_type='^image/webp$' ;;
            *.woff2) deployment_type='^font/woff2$' ;;
            *.txt) deployment_type='^text/plain$' ;;
            *) return 1 ;;
        esac
        deployment_fetch_compare "$deployment_relative" "$deployment_relative" \
            "$deployment_expected_tree/$deployment_relative" "$deployment_cache" \
            "$deployment_type"
    done <"$deployment_manifest"
)

case "$verify_deployment_source_context" in
    production-wrapper)
        verify_marketplace_production_deployment() {
            test "$#" -eq 5 || return 2
            _verify_marketplace_deployment "$1" production "$2" "$3" "$4" "$5"
        }
        ;;
    fixture-runner)
        verify_marketplace_fixture_deployment() {
            test "$#" -eq 5 || return 2
            _verify_marketplace_deployment "$1" fixture-loopback "$2" "$3" "$4" "$5"
        }
        ;;
esac
unset verify_deployment_source_context
