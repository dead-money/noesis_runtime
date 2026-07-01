# Noesis SDK limitations

A few things the Noesis 3.2.13 SDK can't do, or does differently from WPF. Each
one notes what to do instead, so you don't have to find out the hard way.

## Your app has to handle these

Noesis leaves these to the host platform, so there's no SDK call for them.

- **Clipboard.** No clipboard API. Hook the copy and paste events to catch the
  intent, then read or write the OS clipboard yourself (`arboard` works well on
  desktop).
- **Screenshots.** No built-in screenshot call. Render the view into a render
  target your device owns and read the pixels back from it.
- **Listing installed fonts.** The SDK enumerates the faces inside a given font,
  but not the set of installed families. Your font provider decides which
  families it serves, so ask it.
- **Fonts for text measurement.** Measuring text returns zero until a font
  provider (or a fallback) resolves the family to a real face. That's a setup
  step, not a bug.
- **Frame and CPU profiling.** The profiler macros do nothing unless the SDK was
  built against a third-party profiler. Time your own update and render calls.
  Per-frame render stats and the debug overlays (wireframe, overdraw, batch
  coloring) do work.
- **IME composition for CJK (and other) text input.** Only committed codepoints
  reach the view, through `char_input`. There's no composition string or
  candidate-window API, so the pre-edit UI (the underlined in-progress text and
  the candidate list) is the host platform's job. Drive the OS IME yourself and
  feed the final characters once it commits them.

## Different from WPF

These work, just not the way WPF does it.

- **Custom `TypeConverter`s.** You can't register one to turn a string into a
  custom type during XAML load. The `convert_from_string` path and binding-side
  value converters do work.
- **Some dependency-property helpers are value-types only.** Base-value
  read-back, read-only setters, and value coercion handle value, struct, and
  string properties, but not object or brush ones (and coercion only covers a
  class's first 32 properties). Worth knowing if you write custom controls.
- **`FormattedText` is set up at construction.** Font, size, width, alignment,
  trimming, and the rest are constructor arguments, not setters. Changing one
  means rebuilding the layout.
- **SVG parsing gives you shapes, not an image.** `SVG.Parse` returns the overall
  size plus per-shape fill and stroke info you can inspect or feed into your own
  geometry. It isn't an image source you can hand to an `Image`. (`SVGPath`
  parse, bounds, and hit-testing are fully supported.)
- **Animation control runs through `Storyboard`.** Seek, speed, pause, resume,
  and stop live on storyboard actions, not on a standalone clock.
- **Templates are written in XAML, not built in code.** There's no factory for
  assembling a template's visual tree node by node. Parse it from XAML and assign
  it. Styles, data triggers, and selecting templates from Rust all work.
- **No dispatcher queue.** Schedule deferred or cross-thread work through the
  view's timer API instead.
- **Localization substitutes at parse time.** A `Localize`-style markup extension
  resolves each key against the current locale table while the XAML is parsed,
  then bakes the result in (see the `markup` module). There's no live-updating
  binding behind it, so switching language at runtime means re-loading the XAML,
  not watching strings refresh in place.

## Not supported

The SDK doesn't allow these at all.

- **Custom `Brush`/`Geometry`/`Transform` subclasses.** These serialize into the
  GPU render tree, which a custom subclass can't drive. Custom `Freezable`
  subclasses (with their own dependency properties and freeze support) are fine.
  Custom visual effects are a different story — see "Not wrapped yet" below;
  they aren't impossible, just not exposed here yet.
- **Retained or recorded drawings.** There's no drawing object model and no
  drawing-as-a-brush. Immediate-mode drawing is reachable only by overriding a
  custom element's `OnRender`.
- **UI Automation / accessibility.** The 3.2 native SDK has no automation
  peers, no accessibility tree, and no screen-reader surface — this is a gap in
  the SDK itself, not in the bindings, so there's nothing to wrap. If you need
  accessibility you have to build it at the host level around the view.

## Not wrapped yet

The SDK supports these; the bindings just don't expose them today. They're
binding gaps, not hard limits.

- **Blend interactivity behaviors** (`http://schemas.microsoft.com/xaml/behaviors`).
  The SDK's App/Interactivity package isn't compiled into this build, so XAML
  that pulls in `<i:Interaction.Triggers>`/behaviors fails to load. Reach the
  same outcomes from Rust with event subscriptions plus commands, and drive
  visual states with `go_to_state`.
- **`MediaElement` video playback.** Part of the App-framework package, which
  isn't wrapped. There's no Rust-side path to play video through the view.
- **Custom pixel-shader effects.** The SDK does support them via `ShaderEffect`
  and `BrushShader` (`SetPixelShader`), and the render device already carries a
  batch's custom shader pointer — but there's no Rust-side way to author a
  custom `Effect` yet.
