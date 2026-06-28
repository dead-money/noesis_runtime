# TODO — unexposed Noesis SDK surface

This tracks Noesis Native SDK (3.2.13) features **not yet exposed** through the crate's
C ABI (`cpp/noesis_shim.h`) and Rust wrappers — the remaining gap between the full SDK
and what we wrap today.

The goal is to **complete the crate**: cover the whole reachable SDK surface. Sections below
list only outstanding work (finished work is removed, not annotated). The recommended
sequencing is in [Suggested completion order](#suggested-completion-order); things 3.2.13
genuinely cannot do are recorded under [Known SDK limitations](#known-sdk-limitations) so we
don't keep re-discovering them.

## 8. Controls — programmatic access

`Selector` selection (`SelectedIndex`/`SelectedItem`), `ItemsControl.Items` mutation, `RangeBase`
values, `ToggleButton` tri-state `IsChecked`, `Popup`/`Expander` toggles, `ScrollViewer` offsets +
`ScrollTo*`, and `TextBox`/`PasswordBox` selection/caret are exposed (`src/view.rs` Controls §8 +
`cpp/noesis_controls.cpp`). Remaining:

- **Selection.** `Selector::SelectedValue`/`SelectedValuePath`; `TreeView` selection; `ListView` columns.
- **Items.** `ItemContainerGenerator` deep access (container ⇄ item ⇄ index mapping).
- **Popups/overlays.** `ContextMenu`, `ToolTip`/`ToolTipService`.
- **Scrolling.** Direct `IScrollInfo` and the line/page-scroll methods (`LineUp`/`PageDown`/…).
- **`Image` / `MediaElement`-style** source assignment from code.

## 9. Custom types / reflection registration

The runtime type system is broadly wired: element base classes (`Control`/`FrameworkElement`/
`UserControl`/`Panel`/`Decorator`, each a `Rust*` trampoline on a synthetic `TypeClass`), custom DPs
with coercion / `FrameworkPropertyMetadataOptions` / read-only keys / attached registration,
`MeasureOverride`/`ArrangeOverride` layout, custom enums, custom routed events, `Factory` +
`ContentProperty` metadata, custom `IValueConverter`, and reflected plain (non-DO) properties (which
unblocked §3 plain-VM INPC) all ship (`src/classes.rs`, `src/reflection.rs`, `src/plain_vm.rs`).
Remaining:

- **More base classes.** `Freezable` + custom `Brush`/`Effect`/`Geometry`/`Transform` (different lifetime model than the element bases).
- **More custom-DP value types.** Enum / `Point`/`Vector`/`Size`-struct property types.
- **`DependsOn` metadata** attribution (the `ContentProperty` path ships).

## 11. Brushes, transforms, visual properties

- **Brushes.** Remaining: `VisualBrush` (needs a visual source), full `TileBrush` tiling knobs, and `BrushShader`/custom shaders (out-of-scope per README). Done: `SolidColorBrush`, `LinearGradientBrush`/`RadialGradientBrush` + `GradientStop`s, `ImageBrush` (source wiring via an existing `ImageSource*`; building one from pixels needs §12).
- **Transforms.** Remaining: 3D transforms (`Transform3D`, `CompositeTransform3D`, `MatrixTransform3D`). Done: `TranslateTransform`/`ScaleTransform`/`RotateTransform`/`SkewTransform`/`MatrixTransform`/`TransformGroup`/`CompositeTransform` (code-built in `src/transforms.rs`, assigned via `FrameworkElement::set_render_transform`).
- **Effects.** Remaining: custom `ShaderEffect` (`Batch.pixelShader` path — out-of-scope per README). Done: `BlurEffect`, `DropShadowEffect` (in `src/brushes.rs`, assigned via `set_effect`).

## 14. System integration callbacks

From `IntegrationAPI.h`, none are wired:

- **`SetCursorCallback`** (host updates the OS cursor).
- **`SetSoftwareKeyboardCallback`** (show/hide on-screen keyboard — important on console/mobile).
- **`SetOpenUrlCallback`** / `OpenUrl` (hyperlink navigation).
- **`SetPlayAudioCallback`** / `PlayAudio` (UI sound effects).
- **`SetClipboard`**-style data object exchange (via `DataObject`).
- **`SetCulture` / `GetCulture`** (`CultureInfo`) for localization/formatting.

## 15. XAML loading variants

- **`LoadXaml<T>`** typed variants and `GetXamlDependencies` (asset dependency discovery / preloading).
- **Scheme / assembly-scoped providers.** `SetSchemeXamlProvider`, `SetAssemblyXamlProvider`, and the texture/font equivalents.

## 16. Input — finer control

- **Mouse capture.** `Mouse::Capture` / `CaptureMouse` / `ReleaseMouseCapture` on elements.
- **Keyboard state / modifiers** querying, `Keyboard::Focus`, `KeyboardNavigation` (tab order, directional nav, `FocusManager`).
- **Input gestures / bindings.** `KeyBinding`/`MouseBinding`/`InputBinding`, `KeyGesture`/`MouseGesture` (pairs with §4 routed commands).
- **Stylus** events (distinct from touch).
- **Gamepad / focus engagement** navigation modes — important for console.

## 17. Diagnostics & tooling

- **Profiling.** `CpuProfiler`, `ViewStats` debug overlay (the `GetStats` counters are wrapped; the on-screen overlay is not), memory usage queries.
- **Logging** has a handler; structured log levels / categories could be richer.

## 18. Memory / kernel hooks

- **`SetErrorHandler` / `SetAssertHandler`** (route Noesis fatal errors into our logging/panic path) — `NsCore/Error.h`. Good robustness win.
- **`MemoryCallbacks`** (custom allocator integration with the engine's allocator).
- **`Ptr<T>` / `BaseComponent` lifetime helpers** beyond `base_component_release` (AddReference/GetNumReferences for advanced ownership).

---

## Suggested completion order

Phases A–D are complete (core + cheap wins; presentation; custom types + motion; drawing / media /
text) — the §-sections above track only their leftover remainders. What's left, ordered to finish the
crate with the least rework:

**Phase E — platform & finer input.**
1. §14 System integration callbacks (cursor / soft-keyboard / open-url / audio / clipboard / culture).
2. §16 Finer input (mouse capture, `FocusManager`/keyboard nav, input gestures, **gamepad / focus engagement**).

**Phase F — robustness & profiling.**
3. §18 `SetErrorHandler`/`SetAssertHandler` + memory/lifetime hooks, and §17 profiling (`CpuProfiler`, `ViewStats` overlay).

## Known SDK limitations

Recorded so they aren't re-attempted — 3.2.13 doesn't expose these; the workaround is noted.

- **Route-wide `handledEventsToo` (§5).** `UIElement::AddHandler` is 2-arg only in 3.2.13 — no overload to receive already-handled events as the route bubbles/tunnels. Per-element `handled` honoring (already wrapped) is the ceiling.
- **Headless drag / manipulation synthesis (§5).** The typed `DragEventArgs` / `Manipulation*EventArgs` accessors are wrapped and round-trip tested, but the events themselves cannot be *raised* headlessly: a drag needs an OS pointer/drag loop (`DragDrop::DoDragDrop` is exposed and crosses the FFI but has no synchronous/headless completion) and manipulation events are promoted from a multi-frame touch stream under a live render pass. `tests/routed_events_typed_args.rs` drives keyboard-focus events for real and exercises the drag/manipulation accessors by constructing the real arg structs C++-side (under `--features test-utils`). `DataObject.Copying`/`.Pasting` handlers attach/detach but the clipboard copy/paste that fires them is likewise host-driven.
- **`CollectionView` sort / filter / group (§3).** `ICollectionView` here is current-item navigation only — no `SortDescriptions`, `Filter` delegate, `GroupDescriptions`, or `CollectionViewSource::GetDefaultView` ship. Sort/filter/group in Rust before populating the `ObservableCollection`. (Current-item navigation — `MoveCurrentTo*` — *is* available if ever needed.)
- **`PriorityBinding` (§3).** Not in 3.2.13 — the class doesn't exist in the SDK (a WPF feature Noesis omits, like `NavigationCommands`). No workaround; restructure so a single binding with a `FallbackValue` covers the priority case.
- **`TemplateBinding` runtime construction (§3).** `TemplateBindingExtension` exists but is only meaningful inside a `ControlTemplate`; the XAML `{TemplateBinding X}` parse path already works, and the code path is covered by a templated-parent binding (`{Binding RelativeSource={RelativeSource TemplatedParent}}`, already wrapped). A dedicated runtime wrapper would just duplicate that, so it's intentionally not built.
- **`Dynamic`/`StaticResourceExtension` headless read-back (§7).** Both extensions are runtime-constructible (ctor + `SetResourceKey`), but `GetResourceKey()` reads back `null` until `ProvideValue` resolves the key under a live XAML/binding `ValueTargetProvider` (the key is stored internally as a `FixedString`, not as a readable component). With no observable round-trip headlessly, a fail-if-stubbed wrapper isn't possible, so the code path is not exposed; `{DynamicResource}` / `{StaticResource}` remain fully usable from XAML.
- **Code-built template factory trees (§7).** Noesis 3.2.13 ships no `FrameworkElementFactory`, so there is no WPF-style API to assemble a `ControlTemplate`/`DataTemplate` visual tree from programmatic factory nodes. `FrameworkTemplate::SetVisualTree` takes a prototype `Visual`, but the supported authoring path is XAML parse + assign (already wrapped); `DataTemplateSelector`-from-Rust and the `Style` trigger construction surface (`Trigger`/`DataTrigger`/`MultiTrigger`/`EventTrigger`) *are* now exposed (`src/styles.rs`).
- **`CommandManager.RequerySuggested` / `InvalidateRequerySuggested` (§4).** Absent. Use per-command `BaseCommand::RaiseCanExecuteChanged` (already wrapped) to drive enable/disable.
- **`CommandManager` class-level attached events (§4).** 3.2.13 exposes the static `ExecutedEvent`/`CanExecuteEvent` `RoutedEvent`s but no `AddExecutedHandler`/`AddCanExecuteHandler` convenience (a WPF-ism). Per-`CommandBinding` `Executed`/`CanExecute` handlers (wrapped) are the supported path; a global class-level observer isn't wrapped.
- **`NavigationCommands` (§4).** Header doesn't ship (`ApplicationCommands`/`ComponentCommands` do).
- **`GetBaseValue` object form (§2).** No boxed `GetBaseValue`, so the base-value getter covers value/struct/string DPs only, not component/brush DPs.
- **`Dispatcher::BeginInvoke` (§2).** No NsGui dispatcher queue; queued/cross-thread invoke must route through the View timer API (`CreateTimer`, wrapped).
- **Read-only DP value types (§9).** `DependencyObject::SetReadOnlyProperty` is template-only with no boxed object form, so the key-gated read-only setter covers value / struct / string DPs only — not component / brush DPs. (`DependencyPropertyKey` / `RegisterReadOnly` don't exist in 3.2.13; read-only DPs use `PropertyAccess_ReadOnly` + `SetReadOnlyProperty`.)
- **Coerced-property count (§9).** `CoerceValueCallback` carries no DP identity (signature is `(d, baseValue, coercedValue)`), forcing a static pool of per-slot thunk functions. The pool is 32, so only a class's first 32 dependency properties can opt into coercion; coercion is value/struct only (no object/string tags).
- **Custom `TypeConverter` registration (§9).** `TypeConverter::Get` resolves converters through an internal Core registry that runtime `TypeConverterMetaData` + `Factory::RegisterComponent` do not drive (verified: a synthetic converter type registers in the Factory yet `Get` returns null). The *consumption* path (`convert_from_string` via `TryConvertFromString`) and binding-side `IValueConverter` work; string→custom-type conversion during XAML parse is not runtime-registerable.
- **`SVG::Parse` result is a CPU struct, not an `ImageSource` (§12).** `Noesis::SVG::Parse(const char*, SVG::Image&)` fills a plain `SVG::Image` value type (`width`, `height`, `Vector<SVG::Shape>` — each shape an `id`/`data`/fill+stroke `SVG::Brush`/transform record), NOT a `Noesis::ImageSource`/`Drawing` you can assign to an `Image`/`ImageBrush`. There is no SDK path in 3.2.13 to host the parsed result on an element; observe it via its parsed size + shape count + per-shape fill type (`src/svg.rs` `SvgImage`), or feed `SVGPath` command buffers into your own geometry. (`SVGPath` itself — parse, `CalculateBounds`, `FillContains`, `StrokeContains`, and the builder statics — is fully wrapped and headless-testable.)
- **Detached `Clock` / `AnimationClock` controller (§6).** Seek / `SpeedRatio` / `CurrentState` on a standalone (non-`Storyboard`) clock aren't exposed in 3.2.13; use the `Storyboard` controllable actions (Pause/Resume/Stop/Seek) instead.
- **Transform-aware / group `Geometry.GetBounds` (§10).** `Geometry::GetBounds()` reports the *untransformed* path in 3.2.13 — assigning a `Transform` does not move the reported bounds (the assignment is still verifiable via the read-back `GetTransform` pointer). `GeometryGroup`/`PathGeometry` build their aggregate path lazily, so `GetBounds` reads empty until the geometry is rendered in a live view; child / figure / segment counts are the headless FFI-crossing proof. `EllipseGeometry`/`RectangleGeometry`/`LineGeometry`/`StreamGeometry`/`CombinedGeometry` bounds compute eagerly. `StreamGeometryContext` exposes Noesis's actual command set — `CubicTo`/`QuadraticTo`/`SmoothCubicTo`/`SmoothQuadraticTo`/`ArcTo` and `BeginFigure(point, isClosed)` (no per-call `isFilled`/`isStroked`), which differs from WPF's `BezierTo`/`QuadraticBezierTo` naming.
- **`Polygon` / `Polyline` shape elements (§10).** Noesis 3.2.13 ships only `Path`/`Rectangle`/`Ellipse`/`Line` shape elements — there is no `Polygon.h`/`Polyline.h`. Build a polygon/polyline as a `PathGeometry`/`StreamGeometry` (the §10 geometry path) and host it in a `Path`.
- **Offscreen capture / screenshots (§12).** 3.2.13 ships no Noesis API to read back a rendered view's pixels. `IRenderer::Render`/`RenderOffscreen` draw into whatever render target is currently bound on the host `RenderDevice` — capture is purely a host/RenderDevice concern. Workaround: render the view into a render target your `RenderDevice` owns (the `render_offscreen` pass + the wrapped `RenderDevice` render-target surface already give you this) and read the pixels back from that host-side target. (The `IRenderer::Render`/`RenderStereo` family, `flipY`/`clear` flags, and `RenderOffscreen` are the full Noesis-side surface; there is no `SaveToFile` / `ReadPixels` / screenshot entry point.)
- **GPU-resolved imaging values (§12).** `TextureSource::GetTexture` is null, and `BitmapSource::GetPixelWidth/Height` / `GetDpiX/Y` read `0` / defaults, until the source is resolved on a live `RenderDevice` render pass: a real `Noesis::Texture*` is only minted by the host `RenderDevice` (`CreateTexture`), and `BitmapImage` pixel dims resolve through a `TextureProvider` during rendering. The constructible imaging objects (`CroppedBitmap` source + crop rect, `TextureSource`, `BitmapImage` `UriSource`, `DynamicTextureSource` dims) all round-trip headless; the GPU-backed read-backs require driving a host render pass. `DynamicTextureSource`'s `TextureRenderCallback` likewise only fires from the render thread under a live pass. (`BitmapSource::Create(pixels…)` from a raw CPU buffer is also available but still needs a `RenderDevice` to upload before the pixels become a usable `Texture`.)
- **`FormattedText` layout setters (§13).** 3.2.13's `FormattedText` has no `SetMaxTextWidth`/`SetTextAlignment`/`SetFontSize`/… mutators; all constraints (font, size, weight/stretch/style, max width/height, line height, alignment, trimming, flow direction) are *constructor* arguments and metrics are computed once during construction. The wrapper therefore takes them via a builder and rebuilds for a new layout. Getters exposed: `GetBounds`, `GetNumLines`, `GetLineInfo`, `IsEmpty`, `HasVisualBrush`, plus `Measure` and `HitTest`/`GetGlyphPosition`.
- **`FormattedText` glyph positions & standalone `Measure` width (§13).** `GetGlyphPosition` returns `(-10,-10)` and `Measure(...)` reports `0` width for an unconstrained `NoWrap` pass on a `FormattedText` built via the metrics-only ctors — those paths populate measurement/line metrics (`GetBounds`, `GetLineInfo` height/baseline are real) but not the full render layout a `TextBlock` would drive. Glyph-hit geometry needs the object attached to a rendered `TextBlock`; the standalone wrapper exposes the calls but cannot guarantee non-zero render coordinates.
- **`FormattedText` font resolution (§13).** Metrics are only non-zero when the named `FontFamily` resolves to a real face: register a `FontProvider` (or set font fallbacks) before measuring. With no font system configured Noesis cannot shape glyphs and all metrics collapse to zero — this is a configuration dependency, not a stub. `tests/formatted_text.rs` drives the SDK's bundled `Bitter-Regular.ttf` to get genuine metrics.
- **Font-family enumeration (§13).** No SDK API enumerates the set of *available family names* from the font system. `FontFamily` offers per-family enumeration only (`GetNumFonts`/`GetFontName`/`GetFontPath`, resolved through the registered provider — wrapped as `FontFamily::num_fonts`/`font_name`), and `Fonts::GetTypefaces(Stream*, cb)` enumerates the faces inside *one supplied font file*, not the registry. Workaround: the host font provider (`scan_folder` / `register_font`, already wrapped) is the authority on which families it serves, so the host can enumerate its own families.
- **No public Drawing object model / `DrawingVisual::RenderOpen` (§10).** In 3.2.13 `DrawingContext` has a private constructor (`friend UIElement`) and is delivered ONLY to `UIElement::OnRender(DrawingContext*)`; there is no public `DrawingVisual`/`RenderOpen` and no `Drawing`/`DrawingGroup`/`GeometryDrawing`/`ImageDrawing`/`DrawingImage`/`DrawingBrush` headers. So immediate-mode drawing is reachable only by overriding `OnRender` — which the §10 PR wires through a `render` callback on the custom-element trampolines (`ClassBuilder::set_render` → a borrowed `DrawingContext`). Retained/recorded drawings and drawing-as-a-brush are not expressible.
- **`DrawingContext::DrawImage` source (§10).** `DrawImage` needs a live `ImageSource`, which this crate cannot build headlessly yet (TODO §12 imaging); the wrapper accepts a borrowed `ImageSource*` but rejects null. `DrawText`/`DrawMesh` are likewise un-exercisable without a `FormattedText` / `MeshData` builder and are not wrapped.
