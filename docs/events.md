# Widget events

OverCrow sends typed host events, never input packets. Interactive nodes use a
stable ID and may receive only the semantic event allowed for their node type:

- buttons: `clicked`;
- text fields: `value-changed` and `submitted`;
- toggles: `toggled`;
- selections: `selection-changed`;
- supported interactive nodes: bounded `focused` and `hovered` state;
- canvases: bounded normalized `scrolled` and `dragged` values.

The host owns keyboard handling, IME composition, selection, clipboard reads,
pointer coordinates outside the widget, and event targeting. A text field sees
only its resulting bounded value. Clipboard write is a separately declared and
granted host command; clipboard read does not exist.

Passive overlays are click-through and receive zero interaction events. They
may still receive bounded ticks, locale and settings changes, sanitized session
data, HTTP or storage results for prior requests, and subscribed provider data.
A tick is host-owned Unix UTC milliseconds, clamped to be nondecreasing during
one controller lifetime; equal consecutive values are valid. Session elapsed
time is a separate host-provided integer. Neither value gives a component
direct access to a system clock.

`WidgetHarness` exercises the same scoped semantic event shape. It refuses an
ID absent from the current view, a semantic event for the wrong node type, and
every interaction while set to passive mode.
