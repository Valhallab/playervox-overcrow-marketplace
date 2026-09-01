# Validation and review

Validation is split by trust boundary. Hosted CI admits untrusted submissions
without executing them. A maintainer sandbox compiles and tests the affected
widget once. Acceptance, signing, and deployment consume the reviewed artifact
and do not repeat source validation.

## Contributor checks

Run the behavior tests for the package being changed, then build its component
once. For the example widget:

```sh
cargo fmt --all -- --check
cargo test -p hello-widget --locked
cargo build -p hello-widget --release --target wasm32-wasip2 --locked
cargo run -p marketplace-tool --locked -- inspect-component \
  target/wasm32-wasip2/release/hello_widget.wasm
scripts/check-policy.sh
```

`inspect-component` is the single component-model boundary check. It verifies
the supported lifecycle exports and forbidden imports directly on the built
WASM. There is no ignored environment-dependent unit test that repeats this
inspection.

Widget tests should exercise observable state transitions, malformed and
bounded inputs, capability denial, locale fallback, and provider failure. Do
not duplicate parser assertions, assert implementation text, or repeat the
same success case through multiple wrappers.

## Hosted admission

The `pull_request_target` workflow uses reviewed base scripts and treats the
proposed Git tree only as data. Path ownership, forbidden `published/` or
trusted-tool changes, repository shape, metadata, and private-material checks
run before any candidate execution. Hosted CI has no signing, deployment, or
merge authority.

Operational objectives, measured from runner start, are:

| Case | Normal target | Diagnostic deadline |
| --- | ---: | ---: |
| Forbidden path or publication edit | under 30 seconds | 1 minute |
| Valid static admission with warm Cargo cache | under 90 seconds | 5 minutes |
| Obsolete commit on the same PR | cancelled | 30 seconds |

The five-minute workflow timeout is a failure, not permission to fall back to
an unreviewed or unsandboxed path.

## Maintainer review

Create review evidence outside the repository in a private `0700` directory:

```sh
review_parent=/absolute/private/path/review
review_bundle="$review_parent/proposed.bundle"
accepted_base_bundle=/absolute/private/path/current-accepted.bundle
/usr/bin/install -d -m 0700 -- "$review_parent"

scripts/review-revision.sh TRUST_SHA REVIEW_SHA "$review_bundle" \
  "$accepted_base_bundle"
```

Omit `accepted_base_bundle` only for the first reviewed catalog or after an
explicit full invalidation. The wrapper requires a clean checkout at
`TRUST_SHA`, prepares dependencies from that trusted snapshot, materializes the
exact proposed Git object, and runs candidate code only inside the bounded,
networkless sandbox.

The affected build plan is deliberately narrow:

| Change | Component work |
| --- | --- |
| One widget/provider source tree | test and compile that target only |
| Web or documentation only | no widget test or compilation |
| SDK, WIT, shared widget data, toolchain, or unknown shared input | full component review |
| Workspace, lockfile, or target metadata | full component review |

Unchanged `component.wasm` files are copied byte-for-byte from the verified
accepted bundle. They are integrity-checked and rebound into the new catalog,
but they are not recompiled, unit-tested, linted, or component-inspected again.
Shared workspace, lockfile, or target metadata is the exception because the
current monorepo cannot prove that another target's dependency graph is
unchanged. A future per-widget lock format may narrow that invalidation safely.
The changed package receives formatting, native behavior tests, one WASM build,
component inspection, catalog assembly, and static-site verification. Clippy
is not a submission security gate; maintainers may run it when changing trusted
repository code.

Normal warm-cache objectives are under two minutes for web-only reviews, under
five minutes for one widget, and under ten minutes for a deliberate full ABI or
bootstrap review. A useful failure should appear within five minutes; the
outer operational deadline is ten minutes. Exceeding it is a performance defect
to diagnose, not a reason to add another validation pass.

The resulting bundle contains the exact staged repository, a canonical ledger,
the trusted/reviewed revisions, and the reviewed Git tree. Keep it private and
outside repositories, CI artifacts, release output, and project temporary
directories.

## Acceptance and publication

After a protected merge, verify ancestry and identical tree without rerunning
the gate:

```sh
scripts/accept-candidate-revision.sh \
  TRUST_SHA REVIEW_SHA CANDIDATE_SHA "$review_bundle"
```

Acceptance atomically rebinds the bundle receipt from `REVIEW_SHA` to the
protected `CANDIDATE_SHA`; its ledger and component bytes do not change. That
accepted bundle becomes the base evidence for the next submission and the
input to offline publication. `build-production.sh` verifies it before and
after copying, compares the private copy against the ledger, then signs and
verifies the static tree without invoking the component staging compiler.

Promotion should diagnose a tree, ancestry, or bundle mismatch in under ten
seconds. Offline signing and static verification should normally finish within
two minutes and must not exceed five minutes without investigation.

Neither hosted admission nor local review proves live OverCrow installation or
desktop/game behavior. Those remain separate application acceptance checks.
