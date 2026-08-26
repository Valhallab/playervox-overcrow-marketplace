# Rust widget creator guide

The reference SDK builds Rust 2024 components for the exact vendored
`overcrow:extension/widget-v1@1.0.0` world. It targets Rust 1.98 and
`wasm32-wasip2`; the component has no WIT imports and receives no WASI API.

Start from [`examples/hello-widget`](../examples/hello-widget). Implement
`Widget` on a `Default` state type, build native view nodes with `ViewBuilder`,
return commands through `OutputBuilder`, and call `export_widget!` once. The
macro owns one widget instance and exports only `init`, `handle`, and `stop`.
Guest crates must preserve the example's wasm-only `no_std`/`alloc` setup;
linking `std` reintroduces forbidden WASI imports. `Widget: Send` lets the SDK
hold that one instance behind its private safe singleton; it exposes no thread
or process capability.

Each interactive node needs a unique, stable ID. IDs route semantic events to
your state; they are not keyboard shortcuts, DOM IDs, or access to raw input.
Builders reject invalid trees, duplicate IDs, oversized strings, excessive
commands, invalid request IDs, and output beyond host limits. The host repeats
all validation and remains authoritative.

The `WidgetContext` contains only the locale, granted capabilities, bounded
settings, and optional sanitized session data. Do not infer a grant from the
manifest: check the context and handle an unavailable capability. The SDK has
no system clock, randomness, filesystem, environment, socket, subprocess,
clipboard-read, raw-input, or arbitrary-log API.

Translations beyond the declared default locale are optional. Declare the
exact available languages in the manifest, keep the default complete, and use
`LocalizedText` for exact-locale selection with default fallback. See the
[localization policy](localization.md).

The application’s explicit local-unverified installation flow is not yet
user-available. When that integration lands, local packages will receive the
same manifest validation and sandbox as signed packages and will remain
disabled until the user enables them.
