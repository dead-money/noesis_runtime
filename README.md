# dm_noesis_runtime

Rust bindings for the [Noesis GUI Native SDK](https://www.noesisengine.com/) — XAML-driven UI for game engines. Loads `.xaml` scenes, drives the View / IRenderer, exposes a `RenderDevice` trait you implement against your own GPU, and lets you author Rust-backed custom controls + markup extensions that XAML can instantiate by name.

Renderer-agnostic — Bevy 0.18 integration lives in the sibling crate [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy).

> **About this project.** This crate is built for Dead Money's internal game projects and was primarily authored by AI agents (Claude Code) wrapping the Noesis Native SDK behind a narrow hand-written C ABI, with a human engineer directing scope, reviewing output, and steering architecture. It's published for transparency and for use inside Dead Money, not as a polished third-party library. Interfaces will shift, not everything is battle-tested, and documentation leans toward "what would a maintainer need?" rather than "what would a brand-new user expect?". If you adopt it anyway, expect to file issues and read source occasionally.

## You need a Noesis license to use this

This crate links against the Noesis Native SDK, which is closed-source commercial software distributed by Noesis Technologies S.L. under their own EULA. dm_noesis_runtime itself does not redistribute the SDK — you must obtain it separately and point `NOESIS_SDK_DIR` at your install. In practical terms:

- **Every developer building this crate needs the [Noesis Native SDK](https://www.noesisengine.com/) (Indie tier or higher).** The build script reads it from `NOESIS_SDK_DIR` at compile time and links `libNoesis.{so,dll,dylib}` from the appropriate `Bin/<platform>/` subdir.
- **Distribution of binaries built against this crate is governed by your Noesis license.** Indie / Pro / Enterprise have different redistribution terms — see the [Noesis pricing page](https://www.noesisengine.com/pricing.php).
- **The `NOESIS_LICENSE_NAME` / `NOESIS_LICENSE_KEY` env vars suppress the trial watermark.** Without them the runtime works but renders a "trial" banner.

## What's in the box

- **Lifecycle + version.** `init` / `shutdown` / `set_license` / `version`.
- **`RenderDevice` trait.** Implement Noesis's `RenderDevice` virtuals from Rust. The C++ shim ships a `RustRenderDevice` subclass that trampolines every pure virtual into your Rust impl. POD mirrors of `Batch` / `DeviceCaps` / `Tile` / `UniformData` carry the per-draw payload across the boundary; field offsets are asserted at compile time so SDK header drift fails the build.
- **Resource providers.** `XamlProvider` / `FontProvider` / `TextureProvider` traits — same trampoline pattern. The font provider subclasses `CachedFontProvider`, so weight/stretch/style matching stays inside Noesis. The texture provider hands decoded RGBA8 bytes straight to Noesis's `RenderDevice::CreateTexture`.
- **View + Renderer wrappers.** `FrameworkElement::load(uri)` → `View::create(element)` → `Renderer::init(device)` → per-frame `update_render_tree` / `render`. `View::content` + `FrameworkElement::find_name` for named-element lookup.
- **Input pump.** `mouse_move`, `mouse_button_{down,up}`, `mouse_double_click`, `mouse_wheel`, `scroll`, `touch_{down,move,up}`, `key_{down,up}`, `char_input`, `activate` / `deactivate`. `MouseButton` and `Key` enum subsets carry C++ `static_assert` ordinal checks against the SDK's `InputEnums.h` so a future Noesis version that reorders them fails compile rather than silently misroutes events.
- **Routed events.** `events::subscribe_click(&framework_element, handler)` returns an RAII `ClickSubscription`. Other routed events follow the same trampoline pattern as they earn the surface.
- **Custom XAML classes.** `classes::ClassBuilder` lets you register a Rust-backed type (e.g. `<aor:NineSlicer>`) with declared `DependencyProperty`s. The C++ side synthesizes a `TypeClassBuilder` per consumer-named class so XAML's parser, `Style TargetType` matching, and `IsAssignableFrom` all behave normally. Property changes fire a typed `PropertyChangeHandler::on_changed` callback. v1 supports `i32` / `f32` / `f64` / `bool` / `String` / `Thickness` / `Color` / `Rect` / `ImageSource` / `BaseComponent` DP types and `ContentControl` as the trampoline base.
- **Custom MarkupExtensions.** `markup::MarkupExtensionRegistration` registers a Rust callback for `{myns:Foo positional_arg}` syntax. The single `Key` positional arg is wired as the ContentProperty; callbacks return either a string or a `BaseComponent*`. AoR's `LocalizeExtension` is the motivating consumer.

## What's explicitly out of scope

- **Visual studio / Blend integration.** This is a runtime, not a tool — author your XAML in [Noesis Blend](https://www.noesisengine.com/xamltoy/) or VS / Rider with a Noesis plugin and load the resulting files at runtime.
- **WPF/UWP-only XAML features.** Anything not implemented by the Noesis runtime itself: x:Code blocks, x:Static, etc. See the [Noesis XAML reference](https://www.noesisengine.com/docs/Gui.Core.XAMLOverview.html).
- **GPU work in this crate.** Implementing `RenderDevice` is the integrator's job. `dm_noesis_bevy` plugs Noesis into Bevy's wgpu pipeline; if you have your own engine, write your own.
- **Reactive markup-extension bindings.** The current `MarkupExtension` FFI runs the callback at parse time and substitutes the value statically. Locale switches that should update the UI in place need a `LocalizationManager`-style indexer + `Binding` — that's a follow-up.
- **Effects / blur / shadow / opacity pipelines.** Custom pixel-shader registration through `Batch.pixelShader` isn't wired yet.

## Quick start

```toml
[dependencies]
dm_noesis_runtime = { git = "https://github.com/dead-money/dm_noesis_runtime" }
```

```rust
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::{set_xaml_provider, XamlProvider};

// Implement a XAML provider against your asset pipeline.
struct MyXaml(std::collections::HashMap<String, Vec<u8>>);
impl XamlProvider for MyXaml {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

// (Once per process.)
dm_noesis_runtime::init();

// Install the provider. The returned guard owns the registration; drop
// it to clear the global slot.
let provider = MyXaml(/* ... */);
let _guard = set_xaml_provider(provider);

// Load a scene + create a view + drive it from your engine's frame loop.
let element = FrameworkElement::load("MainMenu.xaml")
    .expect("XAML failed to parse");
let mut view = View::create(element);
view.set_size(1920, 1080);
view.activate();
loop {
    // your_input_pump_drives view.mouse_*, view.key_*, view.touch_*
    let _changed = view.update(time_seconds);
    // ... your renderer drives view.get_renderer()
    // see `dm_noesis_bevy` for one complete integration.
}

dm_noesis_runtime::shutdown();
```

For the full pipeline (XAML / font / texture providers, RenderDevice, input forwarding) wired against Bevy 0.18, see [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy).

### Custom controls

```rust
use dm_noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyValue,
};
use dm_noesis_runtime::ffi::{ClassBase, PropType};

struct NineSlicerHandler { source_idx: u32, /* ... */ }
impl PropertyChangeHandler for NineSlicerHandler {
    fn on_changed(&mut self, instance: Instance, idx: u32, _v: PropertyValue<'_>) {
        if idx != self.source_idx { return; }
        let (w, h) = instance.get_image_source_size(self.source_idx)
            .unwrap_or((0.0, 0.0));
        // Compute derived properties, write back via instance.set_rect(...).
    }
}

let mut b = ClassBuilder::new("MyNs.NineSlicer", ClassBase::ContentControl,
                              NineSlicerHandler { source_idx: 0 });
b.add_property("Source", PropType::ImageSource);
b.add_property("SliceThickness", PropType::Thickness);
// ...
let _registration = b.register().expect("class registration failed");

// Now any XAML with `xmlns:my="clr-namespace:MyNs"` can use `<my:NineSlicer/>`.
```

### Custom markup extensions

```rust
use dm_noesis_runtime::markup::MarkupExtensionRegistration;

let table = std::collections::HashMap::from([
    ("menu.greeting", "Hello, world!"),
]);
let _registration = MarkupExtensionRegistration::from_closure(
    "MyNs.Loc",
    move |key| table.get(key).map(|s| s.to_string()),
).expect("markup extension registration failed");

// `{my:Loc menu.greeting}` in XAML now resolves to "Hello, world!".
```

## Design notes

- **No bindgen.** Noesis's public C++ surface (`NsCore`, `NsGui`) leans heavily on templates, intrusive `Ptr<T>` smart pointers, and pure-virtual class hierarchies. Bindgen handles those poorly. We hand-write a narrow C ABI in `cpp/noesis_shim.{h,cpp}` and mirror it in `src/ffi.rs`. The underlying `NsCore` / `NsGui` types stay opaque on the Rust side; only C-layout POD mirrors cross the boundary.
- **POD mirrors with compile-time size assertions.** `Batch`, `Tile`, `DeviceCaps`, `TextureInfo`, etc., are `#[repr(C)]` Rust structs whose layouts mirror the Noesis headers. Each is asserted at compile time with `const _: () = assert!(size_of::<T>() == ...)` so a future SDK that reshapes one of them fails the build instead of silently producing garbage.
- **RAII registration guards.** Every "install something globally in Noesis" entry point (`set_xaml_provider`, `set_font_provider`, `set_texture_provider`, `subscribe_click`, `register_class`, etc.) returns an owning guard that clears the registration on drop. Drop order matters; see the per-module docs for the precise contract.
- **Custom classes via synthetic `TypeClassBuilder`.** Rather than expose `NsImplementReflection` directly (which is template machinery), the C++ shim ships per-base trampoline subclasses (`RustContentControl`, `RustMarkupExtension`) with a hand-rolled `GetClassType()` override that reports a per-name synthetic `TypeClass`. The trampoline forwards `OnPropertyChanged` / `ProvideValue` virtuals to a Rust callback. This is the same architectural shape Noesis's own C# / Unity binding uses for managed code.
- **Threading.** Noesis is not internally thread-safe. Every public API documents which thread it must be called from; in practice the View + Renderer + input pump all run on a single rendering thread. RAII guards are `Send` so resources can be moved between threads, but the underlying calls are still single-threaded.

## Setup

```sh
unzip NoesisGUI-NativeSDK-linux-3.2.12-Indie.zip -d ~/sdks/noesis-3.2.12
export NOESIS_SDK_DIR=~/sdks/noesis-3.2.12
cargo test
```

Optional — apply your Indie credentials to suppress the trial watermark:

```sh
export NOESIS_LICENSE_NAME=...
export NOESIS_LICENSE_KEY=...
```

The `tests/lifecycle.rs` test calls `init` / `version` / `shutdown` and asserts a non-empty version string. `--features test-utils` unlocks `tests/render_device.rs` (full `RenderDevice` trampoline regression).

## Layout

For maintainers — what lives where in the tree.

- `cpp/noesis_shim.{h,cpp}` — narrow C ABI declarations + lifecycle.
- `cpp/noesis_render_device.cpp` — `RustRenderDevice` / `RustTexture` / `RustRenderTarget` subclasses that trampoline Noesis virtuals into the Rust vtable.
- `cpp/noesis_view.cpp` — `RustXamlProvider` subclass + `IView` / `IRenderer` forwarders + the View input trampolines.
- `cpp/noesis_font_provider.cpp` — `RustFontProvider` subclass of `Noesis::CachedFontProvider`.
- `cpp/noesis_texture_provider.cpp` — `RustTextureProvider` subclass; `LoadTexture` turns around and calls `device->CreateTexture(...)`.
- `cpp/noesis_events.cpp` — `RustClickHandler` + `FindName` / `GetName` accessors.
- `cpp/noesis_classes.cpp` — `RustContentControl` trampoline + `TypeClassBuilder` synthesis for custom XAML classes; `ImageSource::GetSize` accessor.
- `cpp/noesis_markup.cpp` — `RustMarkupExtension` trampoline for `{aor:Foo}` syntax.
- `src/ffi.rs` — Rust declarations mirroring the shim header.
- `src/lib.rs` — top-level safe wrappers (lifecycle).
- `src/render_device/` — `RenderDevice` trait + `register()` / `Registered` guard + POD mirrors.
- `src/xaml_provider.rs`, `src/font_provider.rs`, `src/texture_provider.rs` — provider traits.
- `src/view.rs` — `FrameworkElement` + `View` + `Renderer` wrappers; input pump methods.
- `src/events.rs` — `ClickHandler` trait + `subscribe_click` (Phase 5.B).
- `src/classes.rs` — `ClassBuilder` / `ClassRegistration` / `Instance` / `PropertyChangeHandler` (Phase 5.C).
- `src/markup.rs` — `MarkupExtensionRegistration` (Phase 5.D).
- `src/gui.rs` — global `GUI::*` bindings (`LoadApplicationResources`).
- `build.rs` — resolves `NOESIS_SDK_DIR`, compiles the shim TUs with `cc`, links `libNoesis`, bakes `Bin/<platform>/` into rpath on Linux.

## Licensing

Source in this repository is © 2026 Dead Money, distributed under the [MIT License](./LICENSE). Every file under `cpp/`, `src/`, `tests/`, and `docs/` is original work — no Noesis SDK code is vendored or translated, only `#include`'d at compile time from `NOESIS_SDK_DIR`.

The Noesis Native SDK itself is **not redistributed** here. You must obtain it from Noesis Technologies under their own EULA; `build.rs` reads it from `NOESIS_SDK_DIR` at compile time and links `libNoesis.{so,dll,dylib}` from `Bin/<platform>/`. Use, distribution, and licensing of binaries you build that link against the SDK are governed by the Noesis EULA, not by the MIT License above.

## Acknowledgements

Built by wrapping [Noesis Technologies](https://www.noesisengine.com/)' Native SDK. The upstream documentation at [docs.noesisengine.com](https://docs.noesisengine.com/) remains the source of truth for XAML behaviour, control templates, and binding semantics — protocol or runtime bugs in the underlying SDK should be reported there; FFI / wrapper bugs should be filed here.
