# Noesis SDK limitations

This crate exposes the full reachable surface of the Noesis 3.2.13 native SDK.
A handful of things the SDK genuinely can't do — or does differently from WPF —
are recorded here so they aren't repeatedly re-investigated. Each entry notes
what works instead. Items are grouped by what the limitation means for you.

## Works in a running app, not in headless tests

These depend on a live view, render pass, or input loop. They work in a real
application — they just can't be exercised or observed without one, so the
crate's headless tests assert that the accessors round-trip rather than that the
live behavior fires.

- **Tooltip and context-menu open state.** You can read whether a tooltip or
  context menu is open, but *opening* one needs a hosted view with a placement
  target, so the transition only happens in a running app.
- **Drag-and-drop and manipulation gestures.** The drag and manipulation event
  arguments are fully readable, but the events are raised by a real OS
  pointer/touch stream under a render pass and can't be synthesized headlessly.
- **Glyph hit-testing and unconstrained text measurement.** `FormattedText`
  glyph positions and no-wrap measured width resolve only when a rendered
  `TextBlock` lays the text out; line metrics and bounds are available without
  one.
- **Geometry bounds with transforms or groups.** `Geometry.Bounds` reports the
  untransformed shape (an assigned transform doesn't move it), and group/path
  geometries report empty bounds until drawn. Rectangle, ellipse, line, and
  combined geometries compute bounds immediately.
- **Pixel dimensions and textures on image sources.** A bitmap's pixel size,
  DPI, and backing texture resolve only once a render device uploads it; the
  image objects (crop, URI source, dynamic source) otherwise construct and
  round-trip normally.
- **Drawing an image in a custom `OnRender`.** This needs a live image source,
  which can't be built headlessly yet; drawing shapes, geometry, and rectangles
  works.
- **Reading back a code-built `{DynamicResource}`/`{StaticResource}` key.** The
  key reads back only once it resolves in a live binding pass; both extensions
  work fully from XAML.

## Your host application's responsibility

Noesis hands these to the host platform — there's no SDK API because the SDK
doesn't own the resource.

- **Clipboard copy and paste.** There's no clipboard API. Hook the copy/paste
  events to observe intent and read/write the OS clipboard from your host code
  (e.g. `arboard` on desktop).
- **Screenshots and pixel readback.** Render the view into a render target your
  render device owns, then read the pixels back from that target; there's no
  built-in screenshot call.
- **Listing available font families.** The SDK enumerates the faces within a
  given font, but not the set of installed families — your host font provider is
  the authority on which families it serves.
- **Fonts for text metrics.** Text measurement returns zero until a font
  provider (or fallback) resolves the requested family to a real face. This is a
  setup requirement, not a stub.
- **Frame and CPU profiling.** The SDK's profiler macros are inert unless the
  SDK itself was built against a third-party profiler. Time your own
  update/render calls; per-frame render stats and the debug overlays (wireframe,
  overdraw, batch coloring) are available.

## Behaves differently from WPF

These work, but the API shape or the approach differs — use the noted
alternative.

- **`DependsOn` is per-type, not per-property.** It attaches at the type level,
  and only the first record on a type is retrievable.
- **Some dependency-property helpers cover simple value types only.** Base-value
  read-back, read-only setters, and value coercion support value/struct/string
  properties but not object/brush ones; coercion additionally applies to a
  class's first 32 properties.
- **Custom `TypeConverter`s aren't registerable for XAML parsing.** Converting a
  string to a custom type during XAML load isn't supported at runtime; the
  `convert_from_string` path and binding-side value converters work.
- **`FormattedText` is configured at construction.** Font, size, width,
  alignment, trimming, and the rest are constructor arguments rather than
  setters, so the builder rebuilds the layout for a new configuration.
- **SVG parsing yields shape data, not an image.** `SVG.Parse` returns a value
  with the overall size and per-shape fill/stroke info you can inspect or feed
  into your own geometry; it isn't an image source you can assign to an `Image`.
  (`SVGPath` parse, bounds, and hit-testing are fully supported.)
- **Animation control goes through `Storyboard`.** Seek, speed, and state live
  on storyboard actions (pause/resume/stop/seek), not on a standalone clock.
- **Templates are authored in XAML, not assembled in code.** There's no factory
  API to build a template's visual tree node by node; parse the template from
  XAML and assign it. Style/data-trigger construction and selecting templates
  from Rust are supported.
- **Queued and cross-thread work uses the view timer.** There's no dispatcher
  queue; schedule deferred or cross-thread invokes through the view's timer API.

## Intentionally not exposed

These exist in the SDK and work, but the crate deliberately doesn't wrap them.

- **Custom memory allocator.** Routing every Noesis allocation through Rust is
  process-global, must be installed before initialization, and can't be undone —
  high risk for little gain. Read-only memory stats (bytes allocated, allocation
  count) are exposed instead.
- **Runtime `{TemplateBinding}` construction.** It's only meaningful inside a
  control template, where the equivalent templated-parent binding already covers
  it, so a dedicated wrapper would just duplicate that path.

## Not supported by the SDK

The SDK doesn't allow these at all.

- **Custom `Brush`/`Geometry`/`Transform`/`Effect` subclasses.** These render
  types serialize into the GPU render tree, which a custom subclass can't drive,
  so they aren't subclassable. Custom `Freezable` subclasses — with custom
  dependency properties and freeze support — *are* supported.
- **Retained or recorded drawings.** There's no public drawing object model and
  no drawing-as-a-brush; immediate-mode drawing is reachable only by overriding
  a custom element's `OnRender`.
- **Low-level `IScrollInfo` access.** The backend isn't publicly reachable; use
  the public scroll methods (`LineUp`/`PageDown`/`ScrollTo…`).
- **Assert-handler hook on Release builds.** It's compiled out of the Release
  Noesis binary, so it fires only on debug SDK builds. The error-handler hooks
  work on every build.
