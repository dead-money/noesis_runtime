# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). While the crate is
pre-1.0, any `0.x` release may contain breaking changes.

## [Unreleased]

## [0.12.0] - 2026-07-02

### Added

- `ResourceDictionary::set_source(uri)` loads a dictionary's XAML by URI with
  `{StaticResource}` resolving against every scope already reachable from it.
  Join an installed parent's merged dictionaries first and a chain composes
  leaf by leaf: the open-coded `gui::install_app_resources_chain`, for callers
  that also layer code-built entries into the same parent.
- `binding::clear_binding(element, dp_name)` removes a binding wired by
  `set_binding` (`BindingOperations::ClearBinding`); the DP reverts to
  default/local-value precedence. Reports success when no binding was present,
  so removal-driven teardown needs no bound-ness tracking.

### Changed

- **Breaking:** the integration callbacks — `set_cursor_callback`,
  `set_software_keyboard_callback`, `set_open_url_callback`, and
  `set_play_audio_callback` — now require `Fn` closures instead of `FnMut`.
  Noesis can invoke these re-entrantly, so `&mut` access to captured state was
  unsound.
- **Breaking:** the diagnostics handlers — `set_error_handler`,
  `set_assert_handler`, and `set_thread_error_handler` — now require `Fn`
  closures instead of `FnMut`, for the same re-entrancy reason.
- **Breaking:** `get_font_family` now returns `FontFamilyRef<'a>` borrowing the
  `FrameworkElement` it was read from, so the reference can no longer outlive
  the element that owns it.
- **Breaking:** `EventTrigger::source_name` now returns `None` for an unset
  (empty) name instead of `Some("")`.
- The typography enum accessors (font weight/style/stretch, capitals, numeral
  style, fraction, variants, and the composition-underline line style) return
  `None` when the underlying discriminant is unknown, rather than silently
  substituting a default value.
- `Style::builder` now panics if `target_type` is unknown to the reflection
  registry or contains an interior NUL byte — previously it swallowed the error
  and returned an inert builder whose setters all silently failed. Use
  `Style::new` + `set_target_type` for the fallible form.
- Added the gamepad `Key` variants (`GamepadLeft` through `GamepadContext4`,
  ordinals 175–190) so directional-focus and accept/cancel input can be sent
  through `View::key_down` / `key_up`.
- Event-argument classification behind `EventArgs::wheel_delta` is now keyed on
  the arg-kind discriminant reported by the C++ side rather than inferred from
  position/button sentinels, so plain mouse events (moves, enter/leave) no
  longer misreport as zero-delta wheel events.

### Fixed

- Provider registration guards (font, texture, and XAML `Registered`) now clear
  the Noesis slot on drop only when their id still matches the active
  registration, so dropping a stale guard can no longer tear down a newer
  provider for the same scope. Each guard frees exactly its own boxed impl.
- Diagnostics handler guards track a per-registration id: installing a second
  handler leaves the older guard logically dead, and dropping it no longer
  clobbers the newer handler's slot. Each guard restores Noesis's default on
  drop and frees exactly its own closure.
- Event, collection-view current-changed, and lifecycle/data-object
  subscriptions now donate their boxed handler to the C++ side with a free
  trampoline. Dropping a subscription from inside its own callback is safe
  (destruction is deferred until the callback returns) and the previous
  double-free / leak on the Rust side is gone.
- Creating a reflected class instance no longer leaks a reference: the C++ shim
  handed out an object that had been `AddReference`'d twice but released once.
  The freezable make-trampoline path now adopts its `+1` correctly rather than
  taking an extra reference.
- `FontFamilyRef` borrows are tied to the source element's lifetime (see above),
  closing a use-after-free when the element was dropped while the ref was live.
- C++ shim: `noesis_collection_view_source_get_view` now documents that it
  synthesizes a `CollectionView` over the source list for a standalone
  (unhosted) `CollectionViewSource`, matching the implementation. Added
  `static_assert`s pinning the 16 gamepad `Key` ordinals against the SDK enum,
  and switched string-property FFI casts from `i8` to `c_char` for targets
  where `c_char` is `u8` (e.g. aarch64 Linux).

## [0.11.0] - 2026-06-29

### Added

- `ObservableCollection::move_item(old, new)` performs a true positional move on
  the underlying Noesis collection, raising a single `CollectionChanged.Move`
  rather than a Remove+Add. A reorder keeps `ICollectionView` currency (selection)
  and scroll position instead of resetting them.
- `ObservableCollection::insert_object` inserts a reflected object at an index for
  entity-keyed list inserts, without a clear-and-rebuild.
- `u64` reflected values: `PlainType::U64` / `PlainValue::U64`, `ItemValue::U64`,
  `PropType::UInt64`, and `Instance::{set_u64, get_u64}` plus the plain-VM
  `set_u64` / `get_u64` / `as_u64`. Stamp a `u64` (e.g. a host `Entity`'s bits) on
  a reflected row or view-model object as a stable key.
- `FrameworkElement::data_context_u64` and `EventArgs::source_data_context_u64`
  read a `u64` back off an element's (or a routed event source's) `DataContext`,
  resolving an event to the row it originated from with no side channel.
- `subscribe_selection_changed` (with `SelectionChangedHandler` and the RAII
  `SelectionChangedSubscription`) subscribes to `Selector::SelectionChanged`, so a
  control's selection surfaces through `ICollectionView` currency.

## [0.10.0] - 2026-06-29

### Changed

- **Breaking:** command parameters now reach handlers as a decodable
  `CommandParameterValue` instead of a raw `Option<NonNull<c_void>>`. The
  `CommandHandler` and `CommandBindingHandler` trait methods (and their blanket
  `Fn` impls), along with the `execute` / `can_execute` methods on
  `RoutedCommand`, `RoutedUICommand`, and `BorrowedCommand`, now take
  `CommandParameterValue`. The old `CommandParameter` type alias is removed.

### Added

- `CommandParameterValue` decodes the boxed command parameter from XAML (e.g.
  `CommandParameter="42"`): `as_bool`, `as_i32`, `as_f64`, and `as_str` return
  `None` on a type mismatch, plus `is_none` / `raw` for the no-parameter and
  raw-pointer cases. Construct one with `CommandParameterValue::new` to supply a
  parameter when invoking a command yourself.
- Typed `ItemsSource` items: `push_i32`, `push_f64`, `push_bool`, and
  `push_object` add values without boxing them yourself, and `CurrentItem` reads
  them back with `as_i32`, `as_f64`, `as_bool`, and `as_string`.
- `FrameworkElement::remove_input_binding` tears down an input binding added with
  the `add_to` counterpart.
- `View::predict_focus_name` names the element that focus navigation would move
  to in a given direction, when you only need its name.
- `View::solid_brush_color` reads back the RGBA of a named element's
  `SolidColorBrush`.
- `InlineCollection::clear` empties a `TextBlock`'s inlines so they can be
  repopulated without rebuilding the host element.
- `Shape::as_element` views a built shape as an owning `FrameworkElement`.
- `ResourceDictionary::add_brush` inserts a typed brush under a key.
- Windows (`x86_64-pc-windows-msvc`) builds. The build script links `Noesis.lib`
  from the SDK's `Lib/` directory, which the Windows package keeps separate from
  the `Noesis.dll` in `Bin/`, and stages the DLL next to the test and example
  binaries so `cargo test` runs without a `PATH` change.

## [0.9.0]

First public release. The API covers loading XAML, driving the View and
Renderer, implementing a `RenderDevice` against your own GPU, and writing
Rust-backed custom controls and markup extensions. The surface is considered
near-final ahead of a 1.0 that commits to stability.

[Unreleased]: https://github.com/dead-money/noesis_runtime/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/dead-money/noesis_runtime/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/dead-money/noesis_runtime/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/dead-money/noesis_runtime/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/dead-money/noesis_runtime/releases/tag/v0.9.0
