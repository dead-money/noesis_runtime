# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). While the crate is
pre-1.0, any `0.x` release may contain breaking changes.

## [Unreleased]

## [0.10.0]

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

## [0.9.0]

First public release. The API covers loading XAML, driving the View and
Renderer, implementing a `RenderDevice` against your own GPU, and writing
Rust-backed custom controls and markup extensions. The surface is considered
near-final ahead of a 1.0 that commits to stability.

[Unreleased]: https://github.com/dead-money/noesis_runtime/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/dead-money/noesis_runtime/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/dead-money/noesis_runtime/releases/tag/v0.9.0
