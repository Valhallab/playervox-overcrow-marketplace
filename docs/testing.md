# Testing widgets

Run native SDK and widget behavior tests first:

```sh
cargo test -p overcrow-widget-sdk -p hello-widget \
  -p warframe-worldstate-provider -p warframe-widget-data \
  -p warframe-status-widget -p warframe-fissures-widget \
  -p warframe-sortie-archon-widget -p warframe-invasions-widget \
  -p warframe-market-widget --locked
cargo clippy -p overcrow-widget-sdk -p hello-widget \
  -p warframe-worldstate-provider -p warframe-widget-data \
  -p warframe-status-widget -p warframe-fissures-widget \
  -p warframe-sortie-archon-widget -p warframe-invasions-widget \
  -p warframe-market-widget --all-targets --locked -- -D warnings
```

`WidgetHarness` initializes real widget state, routes scoped semantic events,
updates context before locale/settings/session handlers, and exposes the current
rendered view for assertions. Test passive behavior, locale fallback, malformed
or oversized values, unavailable provider/API data, and every state transition.

Synchronize and verify the contract before a component build:

```sh
scripts/sync-wit.sh /absolute/path/to/the/approved/overcrow-worktree
digest=$(cat wit/widget-v1.sha256)
printf '%s  %s\n' "$digest" wit/widget-v1.wit | /usr/bin/sha256sum -c -
cargo build -p hello-widget --release --target wasm32-wasip2 --locked
OVERCROW_HELLO_COMPONENT="$PWD/target/wasm32-wasip2/release/hello_widget.wasm" \
  cargo test -p hello-widget \
  tests::built_component_has_no_imports_and_exact_lifecycle_exports \
  --locked -- --ignored --exact
```

The ignored test uses Bytecode Alliance's component-model parser. It requires
exactly the `init`, `handle`, and `stop` function exports and zero imports,
including zero `wasi:` imports. A normal core-Wasm symbol dumper is not a
substitute for component-model inspection.

Use fixed toolchain/dependency versions, a clean checkout, deterministic input
files, and remapped source paths for reproducible package builds. Validate the
manifest with the matching OverCrow parser and confirm that its capabilities
are exactly the commands the widget can emit. Local test fixtures must never
contain production signing material.

Before opening a widget pull request, also run:

```sh
scripts/check-policy.sh
sh tests/check-policy-smoke.sh
sh tests/ci-policy-smoke.sh
sh tests/ci-trust-boundary-smoke.sh
sh tests/community-submission-smoke.sh
sh tests/sandbox-component-build-smoke.sh
sh tests/sandbox-review-checks-smoke.sh
sh -n scripts/*.sh tests/*.sh
```

Creators may run those commands directly on code they authored. A maintainer
must not run contributor code directly on the host. From a clean checkout of
the exact trusted `candidate` base, first populate the pinned dependency cache,
then run the reviewed full gate against the proposed Git object:

```sh
cargo fetch --locked --manifest-path tools/marketplace-tool/Cargo.toml
scripts/review-revision.sh TRUST_SHA REVIEW_SHA
```

The wrapper requires `HEAD` to equal `TRUST_SHA`, materializes both revisions
from Git rather than the working tree, and invokes `ci-verify.sh` in `full`
mode. Candidate compilation and tests then run only inside the bounded
sandboxes. Missing cached inputs or unsupported sandbox primitives fail closed.

Hosted CI has read-only permissions and no credentials. It materializes the
actual candidate tree and applies trusted-base metadata, path, manifest, and
private-material admission, but exits before staging, compilation, tests, or
any other candidate execution. This keeps public pull requests off a persistent
self-hosted runner and avoids weakening confinement for GitHub-hosted kernels
that reject Bubblewrap's required user namespace.

The full wrapper covers sandboxed native tests and builds, both static-site
suites, deterministic output, and malicious fixtures. A maintainer records that
result before merge or promotion. Neither hosted admission nor the full local
gate has publication authority or constitutes live desktop/game acceptance.
