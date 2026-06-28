# TODO — unexposed Noesis SDK surface

This tracks Noesis Native SDK (3.2.13) features **not yet exposed** through the crate's
C ABI (`cpp/noesis_shim.h`) and Rust wrappers — the remaining gap between the full SDK
and what we wrap today.

The goal is to **complete the crate**: cover the whole reachable SDK surface. Sections below
list only outstanding work (finished work is removed, not annotated). The recommended
sequencing is in [Suggested completion order](#suggested-completion-order); things 3.2.13
genuinely cannot do are recorded under [Known SDK limitations](#known-sdk-limitations) so we
don't keep re-discovering them.

## 3. Data binding

Code-built bindings (`RelativeSource` incl. `FindAncestor`, `BindingExpression` update), Rust value
converters, `MultiBinding` + `IMultiValueConverter`, and plain (non-`DependencyObject`) view models
with `INotifyPropertyChanged` (via §9 reflected plain properties) all ship. Remaining:

- **`PriorityBinding`, `TemplateBinding`** (runtime construction).

## 4. Commands

- **`RoutedCommand` / `RoutedUICommand`** (note: `Create` needs a reflected `TypeClass` owner — see §9).
- **`CommandBinding`** (CanExecute/Executed delegates) attached via `UIElement::GetCommandBindings()`, plus `CommandManager`'s attached `CanExecute`/`Executed` routed events.
- **Built-in command libraries:** `ApplicationCommands`, `ComponentCommands` (static `const RoutedUICommand*` getters).

## 5. Routed events

- **Richer typed arg accessors.** Focus-changed (`old`/`new` element — 2 fields, cheap), manipulation (delta/velocity/inertia — ~6 nested structs), and drag (effects/key-states bitmasks). Currently reachable only as base `RoutedEventArgs` (source/handled).
- **`DragDrop` source side** (`DoDragDrop`) and the copy/paste `DataObject` handlers.

## 6. Animation & timing

`Storyboard` (Begin/Pause/Resume/Stop/Seek), the Double/Color/Thickness/Point From-To animations,
Double/Color `*UsingKeyFrames` (Discrete/Linear/Easing key frames), the easing-function family, the
common `Timeline` knobs, and storyboard-less `begin_on` (`ApplyAnimationClock` equivalent) ship
(`src/animation.rs`, `cpp/noesis_animation.cpp`). Remaining:

- **More animation value types.** `Rect`/`Size`/`Object`/`Matrix`/`Int*` animations + their `*UsingKeyFrames`.
- **`KeySpline`** (spline key frames) and per-animation `HandoffBehavior` on the `Storyboard` path.
- **`BeginStoryboard`** trigger-action wrapper (only useful inside a trigger; code-driven `Begin` covers the rest).

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

## 10. Geometry, shapes, drawing

Only `Path.set_points` is exposed.

- **Geometry construction.** `StreamGeometry`/`StreamGeometryContext`, `PathGeometry` + figures/segments (Line/Bezier/Arc/Poly*), `EllipseGeometry`/`RectangleGeometry`/`LineGeometry`, `CombinedGeometry`, `GeometryGroup`.
- **Shapes.** `Rectangle`/`Ellipse`/`Line`/`Polygon`/`Polyline` property access; `Shape` stroke/fill/`Pen`/`DashStyle`.
- **`DrawingContext`** immediate-mode drawing.

## 11. Brushes, transforms, visual properties

- **Brushes.** Remaining: `VisualBrush` (needs a visual source), full `TileBrush` tiling knobs, and `BrushShader`/custom shaders (out-of-scope per README). Done: `SolidColorBrush`, `LinearGradientBrush`/`RadialGradientBrush` + `GradientStop`s, `ImageBrush` (source wiring via an existing `ImageSource*`; building one from pixels needs §12).
- **Transforms.** Remaining: 3D transforms (`Transform3D`, `CompositeTransform3D`, `MatrixTransform3D`). Done: `TranslateTransform`/`ScaleTransform`/`RotateTransform`/`SkewTransform`/`MatrixTransform`/`TransformGroup`/`CompositeTransform` (code-built in `src/transforms.rs`, assigned via `FrameworkElement::set_render_transform`).
- **Effects.** Remaining: custom `ShaderEffect` (`Batch.pixelShader` path — out-of-scope per README). Done: `BlurEffect`, `DropShadowEffect` (in `src/brushes.rs`, assigned via `set_effect`).

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

- **Profiling.** `CpuProfiler`, `ViewStats` debug overlay (the `GetStats` counters are wrapped; the on-screen overlay is not), memory usage queries.
- **Logging** has a handler; structured log levels / categories could be richer.

## 18. Memory / kernel hooks

- **`SetErrorHandler` / `SetAssertHandler`** (route Noesis fatal errors into our logging/panic path) — `NsCore/Error.h`. Good robustness win.
- **`MemoryCallbacks`** (custom allocator integration with the engine's allocator).
- **`Ptr<T>` / `BaseComponent` lifetime helpers** beyond `base_component_release` (AddReference/GetNumReferences for advanced ownership).

---

## Suggested completion order

Phases A–C are complete (core + cheap wins; presentation; custom types + motion) — the §-sections
above track only their leftover remainders. What's left, ordered to finish the crate with the least
rework:

**Phase D — drawing / media / text.**
1. §10 Geometry & shapes, §12 Media / imaging / render targets, §13 rich text & inlines.

**Phase E — platform & finer input.**
2. §14 System integration callbacks (cursor / soft-keyboard / open-url / audio / clipboard / culture).
3. §16 Finer input (mouse capture, `FocusManager`/keyboard nav, input gestures, **gamepad / focus engagement**).
4. §4 Routed commands (`RoutedCommand`/`CommandBinding`/built-in libraries) — pairs with §16 input bindings; the Rust `ICommand` already covers simple cases, so this is late.

**Phase F — robustness & profiling.**
5. §18 `SetErrorHandler`/`SetAssertHandler` + memory/lifetime hooks, and §17 profiling (`CpuProfiler`, `ViewStats` overlay).

## Known SDK limitations

Recorded so they aren't re-attempted — 3.2.13 doesn't expose these; the workaround is noted.

- **Route-wide `handledEventsToo` (§5).** `UIElement::AddHandler` is 2-arg only in 3.2.13 — no overload to receive already-handled events as the route bubbles/tunnels. Per-element `handled` honoring (already wrapped) is the ceiling.
- **`CollectionView` sort / filter / group (§3).** `ICollectionView` here is current-item navigation only — no `SortDescriptions`, `Filter` delegate, `GroupDescriptions`, or `CollectionViewSource::GetDefaultView` ship. Sort/filter/group in Rust before populating the `ObservableCollection`. (Current-item navigation — `MoveCurrentTo*` — *is* available if ever needed.)
- **`CommandManager.RequerySuggested` / `InvalidateRequerySuggested` (§4).** Absent. Use per-command `BaseCommand::RaiseCanExecuteChanged` (already wrapped) to drive enable/disable.
- **`NavigationCommands` (§4).** Header doesn't ship (`ApplicationCommands`/`ComponentCommands` do).
- **`GetBaseValue` object form (§2).** No boxed `GetBaseValue`, so the base-value getter covers value/struct/string DPs only, not component/brush DPs.
- **`Dispatcher::BeginInvoke` (§2).** No NsGui dispatcher queue; queued/cross-thread invoke must route through the View timer API (`CreateTimer`, wrapped).
- **Read-only DP value types (§9).** `DependencyObject::SetReadOnlyProperty` is template-only with no boxed object form, so the key-gated read-only setter covers value / struct / string DPs only — not component / brush DPs. (`DependencyPropertyKey` / `RegisterReadOnly` don't exist in 3.2.13; read-only DPs use `PropertyAccess_ReadOnly` + `SetReadOnlyProperty`.)
- **Coerced-property count (§9).** `CoerceValueCallback` carries no DP identity (signature is `(d, baseValue, coercedValue)`), forcing a static pool of per-slot thunk functions. The pool is 32, so only a class's first 32 dependency properties can opt into coercion; coercion is value/struct only (no object/string tags).
- **Custom `TypeConverter` registration (§9).** `TypeConverter::Get` resolves converters through an internal Core registry that runtime `TypeConverterMetaData` + `Factory::RegisterComponent` do not drive (verified: a synthetic converter type registers in the Factory yet `Get` returns null). The *consumption* path (`convert_from_string` via `TryConvertFromString`) and binding-side `IValueConverter` work; string→custom-type conversion during XAML parse is not runtime-registerable.
- **Detached `Clock` / `AnimationClock` controller (§6).** Seek / `SpeedRatio` / `CurrentState` on a standalone (non-`Storyboard`) clock aren't exposed in 3.2.13; use the `Storyboard` controllable actions (Pause/Resume/Stop/Seek) instead.
