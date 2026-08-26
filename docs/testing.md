# Testing widgets

Run native SDK and widget behavior tests first:

```sh
cargo test -p overcrow-widget-sdk -p hello-widget --locked
cargo clippy -p overcrow-widget-sdk -p hello-widget --all-targets --locked -- -D warnings
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
