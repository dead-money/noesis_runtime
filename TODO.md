# TODO — unexposed Noesis SDK surface

This tracks Noesis Native SDK (3.2.12) features **not yet exposed** through the crate's
C ABI (`cpp/noesis_shim.h`) and Rust wrappers — the remaining gap between the full SDK
and what we wrap today.

The goal is to **complete the crate**: cover the whole reachable SDK surface. Sections below
list only outstanding work (finished work is removed, not annotated). The recommended
sequencing is in [Suggested completion order](#suggested-completion-order); things 3.2.12
genuinely cannot do are recorded under [Known SDK limitations](#known-sdk-limitations) so we
don't keep re-discovering them.

## 1. View / Renderer

- **Gesture / touch thresholds.** `SetHoldingTimeThreshold`, `SetHoldingDistanceThreshold`, `SetManipulationDistanceThreshold`, `SetDoubleTapTimeThreshold`, `SetDoubleTapDistanceThreshold`, `SetEmulateTouch`.
- **Stereo / VR.** `SetStereoOffscreenScaleFactor`.
- **`Rendering` event.** Per-frame `RenderingEventHandler` delegate (hook before render).
- **Renderer offscreen sizing / glyph cache** and the render-thread split (UpdateRenderTree on render thread vs Update on UI thread).

## 2. Element tree access

- **`Dispatcher` queued invoke.** Cross-thread/queued invoke routes through the View timer API (`CreateTimer`, §1); blocked until those land. (Thread-affinity queries `CheckAccess`/`thread_id` are already wrapped — see also [limitations](#known-sdk-limitations).)
- **`Style` / `RenderTransform` first-class typed wrappers.** Reachable today via the generic component accessors; no dedicated sugar yet.
- **Filtered hit testing.** Only the single-point `VisualTreeHelper::HitTest` is wrapped; the `HitTestFilterCallback` / result-callback overload is not.
- **Standalone `INameScope` / `NameScope` object.** Registration goes through `FrameworkElement::RegisterName`/`UnregisterName`; the freestanding scope object isn't exposed.

## 3. Data binding

- **`IMultiValueConverter` + `MultiBinding`** (runtime construction; `TryConvert` over an array of values).
- **`PriorityBinding`, `TemplateBinding`** (runtime construction).
- **`INotifyPropertyChanged` for plain (non-`DependencyObject`) view models.** Large: Noesis resolves non-DP binding paths only through registered `TypeProperty` reflection (no getter-by-name), so this needs the runtime reflection registration from §9. The `DependencyObject`-backed VM path already covers the notification need.

## 4. Commands

- **`RoutedCommand` / `RoutedUICommand`** (note: `Create` needs a reflected `TypeClass` owner — see §9).
- **`CommandBinding`** (CanExecute/Executed delegates) attached via `UIElement::GetCommandBindings()`, plus `CommandManager`'s attached `CanExecute`/`Executed` routed events.
- **Built-in command libraries:** `ApplicationCommands`, `ComponentCommands` (static `const RoutedUICommand*` getters).

## 5. Routed events

- **Richer typed arg accessors.** Focus-changed (`old`/`new` element — 2 fields, cheap), manipulation (delta/velocity/inertia — ~6 nested structs), and drag (effects/key-states bitmasks). Currently reachable only as base `RoutedEventArgs` (source/handled).
- **`DragDrop` source side** (`DoDragDrop`) and the copy/paste `DataObject` handlers.

## 6. Animation & timing

Nothing here is exposed. Leans on §1 View timers for scheduling.

- **`Storyboard`** Begin/Pause/Resume/Stop/Seek (+ `BeginStoryboard` and the controllable actions).
- **Animation classes** (Double/Color/Point/Thickness/Rect/Size/Object/Matrix/Int*, plus `*UsingKeyFrames`, easing functions, key splines, repeat/handoff behaviors).
- **`Clock` / `AnimationClock` / `Timeline`** control and `ApplyAnimationClock`.

## 7. Styles, resources, templates

`ResourceDictionary` access (create/own, key→component add, borrowed lookup,
merged dictionaries, parse-from-XAML, app-resources set/get, per-element
`Resources` + non-throwing `FindResource`, `RegisterDefaultStyles`), `Style`
from code (target type + setters + `BasedOn`, element assign/read-back), and
template parse+assign (`ControlTemplate` via `SetTemplate`, `DataTemplate` via
the component-DP path, `FrameworkTemplate::FindName`) are wired (`src/resources.rs`,
`src/styles.rs`, `cpp/noesis_resources.cpp`).

- **`Style` triggers** from code: `Trigger`, `DataTrigger`, `EventTrigger`,
  `MultiTrigger` (property triggers + their setters/conditions). The
  `GetTriggers()` collection is reachable; the per-trigger construction surface
  is not built yet.
- **Templates — code-built factories.** `ControlTemplate` / `DataTemplate` /
  `ItemsPanelTemplate` / `HierarchicalDataTemplate` built from a programmatic
  `VisualTree` (factory trees), and `DataTemplateSelector` from Rust. The
  parse-from-XAML + assign path covers the common case today.
- **Dynamic resources.** `DynamicResourceExtension`, `StaticResourceExtension`
  (built-ins; we have custom markup extensions but not these). Reachable from
  XAML already; a code path is low priority.

## 8. Controls — programmatic access

`Selector` selection (`SelectedIndex`/`SelectedItem`), `ItemsControl.Items` mutation, `RangeBase`
values, `ToggleButton` tri-state `IsChecked`, `Popup`/`Expander` toggles, `ScrollViewer` offsets +
`ScrollTo*`, and `TextBox`/`PasswordBox` selection/caret are exposed (`src/view.rs` Controls §8 +
`cpp/noesis_controls.cpp`). Remaining:

- **Selection.** `Selector::SelectedValue`/`SelectedValuePath`; `TreeView` selection; `ListView` columns.
- **Items.** `ItemContainerGenerator` deep access (container ⇄ item ⇄ index mapping).
- **Text.** `FormattedText` (its own large feature).
- **Popups/overlays.** `ContextMenu`, `ToolTip`/`ToolTipService`.
- **Scrolling.** Direct `IScrollInfo` and the line/page-scroll methods (`LineUp`/`PageDown`/…).
- **`Image` / `MediaElement`-style** source assignment from code.

## 9. Custom types / reflection registration

`ClassBuilder` supports a `ContentControl` base plus custom markup extensions. This is also the
prerequisite for §3 plain-VM `INotifyPropertyChanged` (runtime `TypeProperty` registration).

- **More base classes.** `Control`, `FrameworkElement`, `UserControl`, `Panel` (custom layout), `Decorator`, `Freezable`, custom `Brush`/`Effect`/`Geometry`/`Transform`.
- **Custom dependency properties:** more types, `PropertyMetadata` (defaults, coercion, `FrameworkPropertyMetadataOptions` like AffectsMeasure/Render), read-only DPs, `attached` properties.
- **Runtime-reflected plain properties** (`NsProp`-equivalent `TypeProperty` registration) so non-DO Rust VMs become bindable — the missing half of §3 INPC.
- **Custom routed events** registration on Rust-backed types.
- **Custom enums** (`NsRegisterEnum`) usable from XAML.
- **`RegisterComponent` / `Factory`** for arbitrary component types; `NsMeta`/content-property/`DependsOn`/`TypeConverter` metadata.
- **Custom `IValueConverter` / `TypeConverter`** registration.
- **Layout participation.** `MeasureOverride`/`ArrangeOverride` trampolines for true custom panels/controls.

## 10. Geometry, shapes, drawing

Only `Path.set_points` is exposed.

- **Geometry construction.** `StreamGeometry`/`StreamGeometryContext`, `PathGeometry` + figures/segments (Line/Bezier/Arc/Poly*), `EllipseGeometry`/`RectangleGeometry`/`LineGeometry`, `CombinedGeometry`, `GeometryGroup`.
- **Shapes.** `Rectangle`/`Ellipse`/`Line`/`Polygon`/`Polyline` property access; `Shape` stroke/fill/`Pen`/`DashStyle`.
- **`DrawingContext`** immediate-mode drawing.

## 11. Brushes, transforms, visual properties

- **Brushes.** `SolidColorBrush` color set, `LinearGradientBrush`/`RadialGradientBrush` + `GradientStop`s, `ImageBrush`, `VisualBrush`, `TileBrush`, `BrushShader`.
- **Transforms.** `TranslateTransform`/`ScaleTransform`/`RotateTransform`/`SkewTransform`/`MatrixTransform`/`TransformGroup`/`CompositeTransform`; 3D transforms (`Transform3D`, `CompositeTransform3D`).
- **Effects.** `BlurEffect`, `DropShadowEffect`, custom `ShaderEffect` (`Batch.pixelShader` path — noted out-of-scope in README).
- **`RenderOptions`** (per-element bitmap scaling / caching hints).

## 12. Media, imaging, render targets

- **Bitmaps.** `BitmapImage`, `BitmapSource`, `CroppedBitmap`, `DynamicTextureSource`, `TextureSource`/`RenderTexture`.
- **SVG.** `SVG` / `SVGPath` loading.
- **Offscreen capture / screenshots** of a rendered view (beyond the raw `render_offscreen` pass).

## 13. Text & fonts (rich)

- **`TextBlock` inlines.** `Run`/`Span`/`Bold`/`Italic`/`Underline`/`Hyperlink`/`LineBreak`/`InlineUIContainer`.
- **`FormattedText`** measurement/layout.
- **Typography** properties, `FontFamily` enumeration, `TextElement` props, `CompositionUnderline` (IME).

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

- **Profiling.** `CpuProfiler`, `ViewStats` overlay (see §1), memory usage queries.
- **Logging** has a handler; structured log levels / categories could be richer.

## 18. Memory / kernel hooks

- **`SetErrorHandler` / `SetAssertHandler`** (route Noesis fatal errors into our logging/panic path) — `NsCore/Error.h`. Good robustness win.
- **`MemoryCallbacks`** (custom allocator integration with the engine's allocator).
- **`Ptr<T>` / `BaseComponent` lifetime helpers** beyond `base_component_release` (AddReference/GetNumReferences for advanced ownership).

---

## Suggested completion order

Ordered to finish the crate with the least rework — cheap completions and high-leverage
primitives first, big rocks once their prerequisites exist. Each phase is a natural batch.

**Phase A — finish the core + cheap wins.** ✅ Complete (§3 `RelativeSource FindAncestor` +
`BindingExpression` update, §5 non-routed lifecycle events; §1 View timers + typed `RenderFlags` +
`ViewStats` + tessellation quality + `MouseHWheel`; §15 `ParseXaml`/`LoadComponent`; §17 inspector).

**Phase B — presentation.**
4. §7 Styles / resources / templates (`FindResource`, `DataTemplate`/`ControlTemplate` from code) — needed to theme and to render bound collections meaningfully.
5. §8 Controls programmatic access — incremental, per control as screens need it.
6. §11 Brushes / transforms — needed to drive styled visuals from code.

**Phase C — custom types + motion.**
7. §9 Custom types / reflection (more base classes, custom DPs/events/enums, `MeasureOverride`/`ArrangeOverride`, and **runtime-reflected plain properties**). Foundational; also unblocks §3 plain-VM INPC.
8. §3 plain-VM `INotifyPropertyChanged` + `MultiBinding`/`IMultiValueConverter` (after §9 reflection).
9. §6 Animation & timing (after §1 timers).

**Phase D — drawing / media / text.**
10. §10 Geometry & shapes, §12 Media / imaging / render targets, §13 rich text & inlines.

**Phase E — platform & finer input.**
11. §14 System integration callbacks (cursor / soft-keyboard / open-url / audio / clipboard / culture).
12. §16 Finer input (mouse capture, `FocusManager`/keyboard nav, input gestures, **gamepad / focus engagement**).
13. §4 Routed commands (`RoutedCommand`/`CommandBinding`/built-in libraries) — pairs with §16 input bindings; the Rust `ICommand` already covers simple cases, so this is late.

**Phase F — robustness & profiling.**
14. §18 `SetErrorHandler`/`SetAssertHandler` + memory/lifetime hooks, and §17 profiling (`CpuProfiler`, `ViewStats` overlay).

## Known SDK limitations

Recorded so they aren't re-attempted — 3.2.12 doesn't expose these; the workaround is noted.

- **Route-wide `handledEventsToo` (§5).** `UIElement::AddHandler` is 2-arg only in 3.2.12 — no overload to receive already-handled events as the route bubbles/tunnels. Per-element `handled` honoring (already wrapped) is the ceiling.
- **`CollectionView` sort / filter / group (§3).** `ICollectionView` here is current-item navigation only — no `SortDescriptions`, `Filter` delegate, `GroupDescriptions`, or `CollectionViewSource::GetDefaultView` ship. Sort/filter/group in Rust before populating the `ObservableCollection`. (Current-item navigation — `MoveCurrentTo*` — *is* available if ever needed.)
- **`CommandManager.RequerySuggested` / `InvalidateRequerySuggested` (§4).** Absent. Use per-command `BaseCommand::RaiseCanExecuteChanged` (already wrapped) to drive enable/disable.
- **`NavigationCommands` (§4).** Header doesn't ship (`ApplicationCommands`/`ComponentCommands` do).
- **`GetBaseValue` object form (§2).** No boxed `GetBaseValue`, so the base-value getter covers value/struct/string DPs only, not component/brush DPs.
- **`Dispatcher::BeginInvoke` (§2).** No NsGui dispatcher queue; queued/cross-thread invoke must route through the View timer API (`CreateTimer`, §1) once wrapped.
