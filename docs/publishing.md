# Publishing

Community intake is open through pull requests to `candidate`, but merge
acceptance is not publication and makes no security certification.
Minimal-permission hosted CI provides static admission evidence without
executing submitted code.
Human maintainers run the complete sandboxed gate on the exact revision, and a
later repository-local `release/*` pull request to `master` may carry output
from the offline publisher. Creators receive no signing or deployment
credentials.

This repository supplies the minimal-permission check and CODEOWNERS
declarations, but GitHub branch protection, the required check, and required
CODEOWNER review are separate operational configuration. Until those controls
are configured, CI output is evidence rather than stand-alone enforcement of
merge policy.

Configure `overcrow/marketplace-admission/candidate` as the required status for
`candidate`, and `overcrow/marketplace-admission/master` for `master`. Use
strict required status checks so the reviewed head must also be current with
its target branch, and require GitHub Actions as the status source when the
ruleset UI offers that restriction. The trusted `pull_request_target` workflow
serializes runs for the same pull request, publishes `pending` before
validation, then publishes the final result on the exact reviewed head.
Reporting jobs have only `statuses: write`; the verification job has only
`contents: read`. Neither permission can merge, publish, deploy, or sign a
package.

The `pull_request_target` job definition comes from the default branch. It does
not check out the proposed revision: it obtains the exact head through the
fixed public repository URL, verifies the expected commit, and treats that Git
object only as input data. The job materializes bounded private base and
candidate snapshots, compiles the exact target-base marketplace validator
offline, and admits candidate Cargo/target metadata and repository policy
through reviewed parsers. It exits before production staging,
package-manifest validation, compilation, tests, or any other candidate
execution. The complete maintainer gate validates package manifests and
listings before merge. Changes under `.github/`, `scripts`, `tests`, or `tools`
are rejected by this path until a maintainer lands those trusted bytes
separately. The first rollout of a new trusted driver therefore fails closed
until that driver exists in the base commit; CI never falls back to a copy from
the pull-request head.

Before accepting or promoting a submission, a maintainer runs the complete
gate from a clean checkout on a compatible Linux host. That gate uses the same
trusted-base admission, then performs native tests and component compilation
inside the bounded Bubblewrap sandboxes. GitHub-hosted Ubuntu currently blocks
the user-namespace mapping required by this confinement, so hosted CI must not
silently replace it with an unsandboxed build. The commands in
[testing.md](testing.md), including `scripts/review-revision.sh`, are the
operational gate until a disposable compatible runner is available.

One validated source record generates both the human site and machine catalog.
Packages bind exact IDs, versions, digests, and sizes; the catalog is canonical,
monotonic, expiring, and signed only after automated checks plus human approval.

The development fixture key is visibly non-production and may be selected only
by the fixed debug trust path. Production signing must require an explicit
absolute key path. Tooling must never generate, copy, cache, print, or commit a
production private key or passphrase.

The local generation flow is:

```sh
scripts/build-local.sh
cargo run -p marketplace-tool --locked -- verify public/marketplace/v1/catalog.json
```

The script stages the provider first, refreshes its exact digest in the four
dependent manifests, builds the full signed development catalog, and copies
the static site into ignored `/public`. It uses a fixed development timestamp
and sequence state, so a rerun with unchanged inputs reproduces the same
objects. A changed payload requires a strictly higher development sequence;
never reset or reuse one. Source package directories never retain
`component.wasm` after publication.

The reviewed offline publisher is `scripts/build-production.sh`. It remains
disabled until the production ceremony commits the reviewed public key at
`keys/overcrow-production-2026-01.pub`; no private authority material belongs
in this repository. It accepts only an exact clean `release/*` commit and
external private files with the required ownership and modes, stages and
verifies the complete tree, advances the sequence, and atomically replaces
`published/`. This is a local build operation and does not deploy or push. The
first catalog expires exactly 30 days after issuance and maintainers republish
at least every 14 days.

Production verification also requires Bubblewrap, a user systemd manager for
transient resource-limited services, and a canonical regular Node executable
selected from `PATH`. The executable and every directory in its resolved path
must be root-owned and not group- or world-writable; the executable must have
mode `0755` and be single-link. A user-managed
version-manager shim is intentionally rejected. The Node checks run without
network and with a `/proc` view limited to their isolated PID namespace, under
fixed CPU, task, virtual-address, resident memory, swap, file, and wall-time
limits. Release and full-gate hosts must provide that system Node installation
or production verification fails closed.
Bubblewrap exposes the host network only to the fixed trusted setup command;
that command creates an empty network namespace, drops every capability, and
sets `no_new_privs` before starting any reviewed package or site code.

The deployment contract serves tracked production output at
<https://overcrow.playervox.com/>, with the marketplace at
<https://overcrow.playervox.com/marketplace/> and the signed catalog at
<https://overcrow.playervox.com/marketplace/v1/catalog.json>. The website
cannot install packages; installation remains a Control Center operation.

Do not add a private key, passphrase, private key path, deployment credential,
or publishing endpoint to local configuration, generated output, CI logs, or
a commit.

Production operation additionally requires the security gates in
[SECURITY.md](../SECURITY.md), a key-operations and recovery procedure, and an
incident suspension/revocation runbook. Deployment remains a separate,
explicitly authorized operation.
