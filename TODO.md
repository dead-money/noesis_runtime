# TODO — unexposed Noesis SDK surface

This tracks Noesis Native SDK (3.2.12) features that are **not yet exposed** through the
crate's C ABI (`cpp/noesis_shim.h`) and Rust wrappers. It's a map of the gap between the
full SDK and what we wrap today, roughly ordered by how likely we are to want each one.

Scope is driven by Dead Money's needs, so most of this is intentionally unbuilt. The point
of the list is to know what exists before deciding to wrap it, not to imply everything here
is planned.

## Already exposed (for reference)

Lifecycle (`init`/`shutdown`/`set_license`/`set_log_handler`/`version`); `RenderDevice` trait;
XAML/font/texture providers (+ font fallbacks, registration, default properties);
`GUI::LoadXaml`, `LoadApplicationResources`, app-resource chain; View create/size/scale/
projection/flags/update + renderer init/update/render/offscreen; full input pump
(mouse/wheel/scroll/touch/key/char/activate); `FrameworkElement` find-by-name, get-name,
visibility, margin; routed `Click` + `KeyDown` subscriptions; TextBox text get/set + caret;
focus; `Path` points; generic name-keyed `DependencyProperty` get/set on any
`DependencyObject` (all 10 FFI value types, with type-tag validation + read-only guard);
`VisualStateManager::GoToState`; custom classes (`ContentControl` base) + custom markup
extensions.

---

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

Building on the generic name-keyed property accessors, the remaining tree-access surface:

- **Attached properties.** Same `FindDependencyProperty` mechanism but needs owner-type-qualified name resolution (e.g. `Grid.Row`, `Canvas.Left`).
- **`ClearValue` / `SetCurrentValue` / `GetBaseValue`** and animation/expression destinations — the generic path only does plain local `SetValue`/`GetValue`.
- **Dynamic tag inference.** A fully dynamic getter that infers the FFI tag from `dp->GetType()` (would need a `Type*`→tag table) rather than the caller supplying it.
- **`FrameworkElement` common props as first-class typed wrappers** (optional sugar over the name-keyed accessors): `ActualWidth`/`ActualHeight`, `HorizontalAlignment`/`VerticalAlignment`, `RenderTransform`, `DataContext`, `Tag`, `Style`, `IsEnabled`, `Focusable`.
- **`DataContext`** set/get (prerequisite for any binding-driven workflow).
- **Tree traversal.** `VisualTreeHelper` (GetChild/GetParent/GetChildrenCount/HitTest), `LogicalTreeHelper`, `FrameworkElement::GetParent`/`GetTemplateChild`.
- **Name scopes.** `INameScope` register/unregister; `FindName` exists, register does not.
- **`Dispatcher`** / `DispatcherObject` thread-affinity helpers (BeginInvoke onto the UI thread).

## 3. Data binding

Entirely unexposed. This is the largest functional area.

- **`Binding`** construction + `SetBinding` / `BindingOperations` (Path, Source, ElementName, RelativeSource, Mode, Converter, StringFormat, FallbackValue).
- **`INotifyPropertyChanged`** implemented from Rust so Rust-backed view models can drive XAML.
- **`ObservableCollection` / `INotifyCollectionChanged`** so list controls update from Rust data.
- **Value converters.** `IValueConverter` / `IMultiValueConverter` implemented in Rust.
- **`MultiBinding`**, `PriorityBinding`, `TemplateBinding` (runtime construction).
- **`CollectionView` / `CollectionViewSource`** (sorting/filtering/grouping/current-item).
- **`RelativeSource`**, `UpdateSourceTrigger`, `BindingExpression` inspection.

## 4. Commands

- **`ICommand` from Rust** (so XAML `Command="{Binding ...}"` can invoke Rust logic).
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

If we wrap nothing else, the two changes that unlock the most are:

1. **Generic `DependencyProperty` get/set** (§2) — removes the need for bespoke per-property
   accessors and is the foundation for almost everything else.
2. **A binding bridge** (§3) — `INotifyPropertyChanged` + `ObservableCollection` from Rust,
   so XAML can be data-driven instead of poked imperatively.

Generic `AddHandler` routed events (§5) are the next most broadly useful, low-surface-area win.
