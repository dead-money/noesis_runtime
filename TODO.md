# TODO — unexposed Noesis SDK surface

This tracks Noesis Native SDK (3.2.12) features that are **not yet exposed** through the
crate's C ABI (`cpp/noesis_shim.h`) and Rust wrappers. It's a map of the gap between the
full SDK and what we wrap today, roughly ordered by how likely we are to want each one.

Scope is driven by Dead Money's needs, so most of this is intentionally unbuilt. The point
of the list is to know what exists before deciding to wrap it, not to imply everything here
is planned.

## 1. View / Renderer

Most of `IView` is unexposed beyond the basics. From `NsGui/IView.h`:

- **Quality / AA tuning.** `SetTessellationMaxPixelError` (Low/Medium/HighQuality), `GetTessellationMaxPixelError`. We set flags but never expose tessellation quality.
- **`RenderFlags` as typed options.** We pass a raw `u32` to `set_flags`; expose the enum (`Wireframe`, `ColorBatches`, `Overdraw`, `FlipY`, `PPAA`, `LCD`, `ShowGlyphs`, `ShowRamps`, `DepthTesting`) and `GetFlags`.
- **Gesture / touch thresholds.** `SetHoldingTimeThreshold`, `SetHoldingDistanceThreshold`, `SetManipulationDistanceThreshold`, `SetDoubleTapTimeThreshold`, `SetDoubleTapDistanceThreshold`, `SetEmulateTouch`.
- **Stereo / VR.** `SetStereoOffscreenScaleFactor`.
- **`Rendering` event.** Per-frame `RenderingEventHandler` delegate (hook before render).
- **View-driven timers.** `CreateTimer` / `RestartTimer` / `CancelTimer` (animation/dispatcher-style callbacks scheduled by the view).
- **`ViewStats`.** `GetStats()` returns frame/update/render time, triangle/draw/batch counts, glyph stats, etc. Useful for profiling overlays.
- **`MouseHWheel`** (horizontal wheel) is in `IView` but not pumped.
- **Renderer offscreen sizing / glyph cache** and render-thread split (UpdateRenderTree on render thread vs Update on UI thread) are not modeled separately.

## 2. Element tree access (DependencyObject / generic properties)

Generic name-keyed property access, attached properties (owner-qualified, incl. `uint32`
layout props like `Grid.Row`), visual + logical tree traversal, single-point hit testing,
name-scope register/unregister, value-source helpers (`ClearLocalValue` / `SetCurrentValue` /
`GetBaseValue`), `Type*`→tag inference, and typed `FrameworkElement` sugar (`ActualWidth/Height`,
`Width/Height/Opacity`, `IsEnabled`, `Focusable`, `Tag`, alignment, `DataContext`) are wrapped.
Remaining:

- **`Dispatcher` queued invoke.** `CheckAccess` / thread-id affinity queries are exposed, but `BeginInvoke`-style cross-thread marshalling has no NsGui surface — it routes through the View's timer API (`CreateTimer`, §1).
- **Base value for reference-typed properties.** `GetBaseValue` has no boxed/object form in the SDK, so the base-value getter covers value/struct/string tags only, not component/brush DPs.
- **`Style` / `RenderTransform` first-class typed wrappers.** Reachable today via the generic component accessors; no dedicated sugar yet.
- **Filtered hit testing.** Only the single-point `VisualTreeHelper::HitTest` is wrapped; the `HitTestFilterCallback` / result-callback overload is not.
- **Standalone `INameScope` / `NameScope` object.** Registration goes through `FrameworkElement::RegisterName`/`UnregisterName`; the freestanding scope object isn't exposed.

## 3. Data binding

`DataContext`, `ObservableCollection` → `ItemsSource`, and `DependencyObject`-backed
view models are wrapped; bindings can be authored in XAML against Rust data. Still missing:

- **`Binding`** construction from code + `SetBinding` / `BindingOperations` (Path, Source, ElementName, RelativeSource, Mode, Converter, StringFormat, FallbackValue) — bindings are XAML-authored only today.
- **`INotifyPropertyChanged`** from Rust for plain (non-`DependencyObject`) view models — needs runtime-reflected properties.
- **Value converters.** `IValueConverter` / `IMultiValueConverter` implemented in Rust.
- **`MultiBinding`**, `PriorityBinding`, `TemplateBinding` (runtime construction).
- **`CollectionView` / `CollectionViewSource`** (sorting/filtering/grouping/current-item).
- **`RelativeSource`**, `UpdateSourceTrigger`, `BindingExpression` inspection.

## 4. Commands

- **`RoutedCommand` / `RoutedUICommand`**, `CommandBinding`, `CommandManager` (RequerySuggested).
- Built-in command libraries: `ApplicationCommands`, `ComponentCommands`, `NavigationCommands`.

## 5. Routed events (beyond Click / KeyDown)

We wrap only `Click` and `KeyDown`. The general mechanism and most events are missing.

- **Generic `AddHandler` / `RemoveHandler`** by `RoutedEvent` (with `handledEventsToo`).
- **Mouse:** `MouseEnter`/`Leave`, `MouseDown`/`Up`, `MouseMove`, `MouseWheel`, `PreviewMouse*`.
- **Keyboard:** `KeyUp`, `PreviewKeyDown`/`Up`, `TextInput`.
- **Focus:** `GotFocus`/`LostFocus`, `GotKeyboardFocus`/`Lost`, `IsKeyboardFocusWithinChanged`.
- **Lifecycle:** `Loaded`/`Unloaded`, `Initialized`, `SizeChanged`, `LayoutUpdated`.
- **Touch / manipulation:** `TouchDown`/`Move`/`Up`, `ManipulationStarting`/`Delta`/`Completed`, `Holding`, `Tapped`.
- **Drag/drop:** `DragEnter`/`Over`/`Leave`/`Drop` (+ `DragDrop` / `DataObject`).

## 6. Animation & timing

Nothing in this area is exposed.

- **`Storyboard`** Begin/Pause/Resume/Stop/Seek (+ `BeginStoryboard` and the controllable actions).
- **Animation classes** (Double/Color/Point/Thickness/Rect/Size/Object/Matrix/Int*, plus `*UsingKeyFrames`, easing functions, key splines, repeat/handoff behaviors).
- **`Clock` / `AnimationClock` / `Timeline`** control and `ApplyAnimationClock`.

## 7. Styles, resources, templates

Only `LoadApplicationResources` is wired.

- **`ResourceDictionary` access.** `GUI::SetApplicationResources` / `GetApplicationResources`, per-element `Resources`, `FindResource` / `TryFindResource`, merged dictionaries, `RegisterDefaultStyles`.
- **`Style`** construction/assignment, setters, triggers (`Trigger`, `DataTrigger`, `EventTrigger`, multi-triggers) from code.
- **Templates.** `ControlTemplate` / `DataTemplate` / `ItemsPanelTemplate` / `HierarchicalDataTemplate` runtime creation; `DataTemplateSelector` from Rust; `FrameworkTemplate::FindName`.
- **Dynamic resources.** `DynamicResourceExtension`, `StaticResourceExtension` (we have custom markup extensions but not these built-ins).

## 8. Controls — programmatic access

We never touch specific control APIs except TextBox text. Each common control has state worth
exposing (selection, value, items, checked, expansion). High-value candidates:

- **Selection.** `Selector` / `ListBox` / `ComboBox` / `TabControl` `SelectedIndex`/`SelectedItem`/`SelectedValue`; `ListView`/`TreeView` selection.
- **Items.** `ItemsControl::GetItems` add/remove/clear, `ItemsSource`, `ItemContainerGenerator`.
- **Ranges.** `RangeBase` (`Slider`, `ProgressBar`, `ScrollBar`) `Value`/`Minimum`/`Maximum`.
- **Toggles.** `ToggleButton`/`CheckBox`/`RadioButton` `IsChecked`.
- **Text.** `PasswordBox`, `TextBox` selection/caret beyond end, `BaseTextBox` selection range, `FormattedText`.
- **Popups/overlays.** `Popup` IsOpen, `ContextMenu`, `ToolTip`/`ToolTipService`, `Expander` IsExpanded.
- **Scrolling.** `ScrollViewer` offsets / `IScrollInfo`, `ScrollToHorizontalOffset` etc.
- **`Image` / `MediaElement`-style** source assignment from code.

## 9. Custom types / reflection registration

`ClassBuilder` supports a `ContentControl` base plus custom markup extensions. The reflection
system supports far more.

- **More base classes.** `Control`, `FrameworkElement`, `UserControl`, `Panel` (custom layout — Measure/Arrange overrides), `Decorator`, `Freezable`, custom `Brush`/`Effect`/`Geometry`/`Transform`.
- **Custom dependency properties:** more types, `PropertyMetadata` (defaults, coercion, `FrameworkPropertyMetadataOptions` like AffectsMeasure/Render), read-only DPs, `attached` properties.
- **Custom routed events** registration on Rust-backed types.
- **Custom enums** (`NsRegisterEnum`) usable from XAML.
- **`RegisterComponent` / `Factory`** for arbitrary component types; `NsMeta`/content-property/`DependsOn`/`TypeConverter` metadata.
- **Custom `IValueConverter` / `TypeConverter`** registration.
- **Layout participation.** `MeasureOverride`/`ArrangeOverride` trampolines for true custom panels/controls (current `ContentControl` base does not expose layout).

## 10. Geometry, shapes, drawing

Only `Path.set_points` is exposed.

- **Geometry construction.** `StreamGeometry` / `StreamGeometryContext`, `PathGeometry` + figures/segments (Line/Bezier/Arc/Poly*), `EllipseGeometry`/`RectangleGeometry`/`LineGeometry`, `CombinedGeometry`, `GeometryGroup`.
- **Shapes.** `Rectangle`/`Ellipse`/`Line`/`Polygon`/`Polyline` property access; `Shape` stroke/fill/`Pen`/`DashStyle`.
- **`DrawingContext`** immediate-mode drawing.

## 11. Brushes, transforms, visual properties

- **Brushes.** `SolidColorBrush` color set, `LinearGradientBrush`/`RadialGradientBrush` + `GradientStop`s, `ImageBrush`, `VisualBrush`, `TileBrush`, `BrushShader`.
- **Transforms.** `TranslateTransform`/`ScaleTransform`/`RotateTransform`/`SkewTransform`/`MatrixTransform`/`TransformGroup`/`CompositeTransform`; 3D transforms (`Transform3D`, `CompositeTransform3D`).
- **Effects.** `BlurEffect`, `DropShadowEffect`, custom `ShaderEffect` (noted out-of-scope in README — `Batch.pixelShader` path).
- **`RenderOptions`** (per-element bitmap scaling / caching hints).

## 12. Media, imaging, render targets

- **Bitmaps.** `BitmapImage`, `BitmapSource`, `CroppedBitmap`, `DynamicTextureSource`, `TextureSource`/`RenderTexture` (render UI into a texture / use a texture as a source).
- **SVG.** `SVG` / `SVGPath` loading.
- **Offscreen capture / screenshots** of a rendered view (beyond the raw `render_offscreen` pass).

## 13. Text & fonts (rich)

- **`TextBlock` inlines.** `Run`/`Span`/`Bold`/`Italic`/`Underline`/`Hyperlink`/`LineBreak`/`InlineUIContainer`.
- **`FormattedText`** measurement/layout.
- **Typography** properties, `FontFamily` enumeration, `TextElement` props, `CompositionUnderline` (IME).

## 14. System integration callbacks

From `IntegrationAPI.h`, none are wired:

- **`SetCursorCallback`** (host updates the OS cursor).
- **`SetSoftwareKeyboardCallback`** (show/hide on-screen keyboard for text fields — important on console/mobile).
- **`SetOpenUrlCallback`** / `OpenUrl` (hyperlink navigation).
- **`SetPlayAudioCallback`** / `PlayAudio` (UI sound effects).
- **`SetClipboard`**-style data object exchange (via `DataObject`).
- **`SetCulture` / `GetCulture`** (`CultureInfo`) for localization/formatting.

## 15. XAML loading variants

- **`ParseXaml`** (parse from an in-memory string, not just a URI).
- **`LoadComponent`** (load XAML into an existing component instance — code-behind pattern).
- **`LoadXaml<T>`** typed variants and `GetXamlDependencies` (asset dependency discovery / preloading).
- **Scheme / assembly-scoped providers.** `SetSchemeXamlProvider`, `SetAssemblyXamlProvider`, and the texture/font equivalents.

## 16. Input — finer control

- **Mouse capture.** `Mouse::Capture` / `CaptureMouse` / `ReleaseMouseCapture` on elements.
- **Keyboard state / modifiers** querying, `Keyboard::Focus`, `KeyboardNavigation` (tab order, directional nav, `FocusManager`).
- **Input gestures / bindings.** `KeyBinding`/`MouseBinding`/`InputBinding`, `KeyGesture`/`MouseGesture`.
- **Stylus** events (distinct from touch).
- **Gamepad / focus engagement** navigation modes.

## 17. Diagnostics & tooling

- **Inspector / hot-reload.** `DisableHotReload`, `DisableInspector`, `IsInspectorConnected`, `UpdateInspector`, `DisableSocketInit` — currently not surfaced (we may want to *enable* the inspector for debugging).
- **Profiling.** `CpuProfiler`, `ViewStats` overlay (see §1), memory usage queries.
- **Logging** has a handler; structured log levels / categories could be richer.

## 18. Memory / kernel hooks

- **`MemoryCallbacks`** (custom allocator integration with the engine's allocator).
- **`SetErrorHandler` / `SetAssertHandler`** (route Noesis fatal errors into our logging/panic path) — `NsCore/Error.h`.
- **`Ptr<T>` / `BaseComponent` lifetime helpers** beyond the single `base_component_release` (AddReference/GetNumReferences if needed for advanced ownership).

---

### Notes on prioritization

The two foundations are now in place: generic `DependencyProperty` get/set plus the broader
element-tree surface (§2), and the binding bridge (§3, `DataContext` + `ObservableCollection`
from Rust). With those wrapped, the next most broadly useful, low-surface-area win is
**generic `AddHandler` / `RemoveHandler` routed events** (§5) — it unlocks every mouse, keyboard,
focus, and lifecycle event from one mechanism instead of bespoke per-event wrappers.
