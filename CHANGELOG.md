# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). While the crate is
pre-1.0, any `0.x` release may contain breaking changes.

## [Unreleased]

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

[Unreleased]: https://github.com/dead-money/noesis_runtime/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/dead-money/noesis_runtime/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/dead-money/noesis_runtime/releases/tag/v0.9.0
