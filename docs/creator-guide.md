# Rust widget creator guide

The reference SDK builds Rust 2024 components for the exact vendored
`overcrow:extension/widget-v1@1.0.0` world. It targets Rust 1.98 and
`wasm32-wasip2`; the component has no WIT imports and receives no WASI API.

Start from [`examples/hello-widget`](../examples/hello-widget). Implement
`Widget` on a `Default` state type, build native view nodes with `ViewBuilder`,
return commands through `OutputBuilder::new(context)`, and call
`export_widget!` once. The
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

Build each output from the callback's mutable context. This preserves monotonic
request IDs, outstanding HTTP/storage slots, and provider revisions across
callbacks; matching result events release their request slot.

The `WidgetContext` contains only the locale, granted capabilities, bounded
settings, and optional sanitized session data. Do not infer a grant from the
manifest: check the context and handle an unavailable capability. The SDK has
no system clock, randomness, filesystem, environment, socket, subprocess,
clipboard-read, raw-input, or arbitrary-log API.

## Views, events, and capabilities

The host renders a bounded native view tree. Available nodes are rows,
columns, grids, scroll regions, text, host icons, bounded raster images,
buttons, toggles, text inputs, selections, lists, progress indicators, charts,
and bounded 2D canvases. Every interactive node needs a stable ID. The host
can emit `clicked`, `value-changed`, `submitted`, `selection-changed`,
`toggled`, `focused`, `hovered`, `scrolled`, and `dragged` only where the node
type permits them; see [events.md](events.md). Passive overlays receive no
interaction events.

Declare only the capabilities the package can use: brokered HTTPS `GET` access
to exact hosts, the reviewed `overcrow.session.v1` game-data feed, bounded
private storage, clipboard write, and provider publication. All are denied
until the host and user grant them. Components never receive direct network,
filesystem, process, raw input, game-memory, or clipboard-read access.

## Package and language limits

Manifests and listings are each limited to 64 KiB; components to 4 MiB;
packages to 16 MiB and 64 entries; raster assets to 8 MiB compressed and
32 MiB decoded in total; and a preview to 256 KiB. A manifest can declare up
to 32 locales, games, and dependencies and up to 16 HTTPS hosts. A listing
must provide exactly one entry for every declared locale, and its canonical
HTTPS source URL is limited to 512 ASCII bytes. Declare the exact
`availableLocales`, including both `en` and the required `defaultLocale`. The
host chooses the application locale when available and otherwise uses the
default; missing strings also fall back to that default. Official Warframe
packages provide `en` and `fr`.

Community submissions require complete English metadata. Other translations
are optional. Declare every supplied language exactly in the manifest and
listing, keep the default complete, and use `LocalizedText` for exact-locale
selection with default fallback. The marketplace always displays the available
locale list; see the [localization policy](localization.md).

The Control Center can install a local package only after an explicit
unverified-development confirmation. Local packages receive the same manifest,
archive, and sandbox validation as signed packages and remain disabled until
the user enables them. The website never installs packages.

## Submit for review

Place the complete source tree at `community/<publisher>/<widget-id>/`, work
from a fork or short-lived branch, and open a pull request to `candidate` using
the widget submission template. Include tests, preview, licenses, provenance,
capability reasons, exact HTTPS hosts, game scope, dependencies, and every
available locale. Minimal-permission hosted CI supplies static admission
evidence without executing the submission; a maintainer runs the sandboxed
build gate and reviews the exact revision. Merge acceptance does not publish
the widget. Publication is a later offline promotion, and every update is
reviewed again. An unrelated submission does not rebuild or retest an already
accepted widget; only changes to that widget or a shared SDK, WIT, or toolchain
input invalidate its evidence.

The Rust SDK is the current authoring path. A later visual builder will target
the same manifest, component, capability, and review contracts rather than a
separate trust path.

For local catalog development, build all components, run
`scripts/build-local.sh`, verify `public/marketplace/v1/catalog.json`, then
serve `public` on loopback. Debug OverCrow builds can browse the signed
development catalog at the fixed numeric-loopback origin; release builds reject
that origin. Separately, the Control Center accepts an individual local package
only from its native file picker. Do not produce or use a production-signed
package from this repository.
