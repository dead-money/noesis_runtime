# noesis_runtime

[![CI](https://github.com/dead-money/noesis_runtime/actions/workflows/ci.yml/badge.svg)](https://github.com/dead-money/noesis_runtime/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/noesis_runtime.svg)](https://crates.io/crates/noesis_runtime)
[![docs.rs](https://docs.rs/noesis_runtime/badge.svg)](https://docs.rs/noesis_runtime)

Rust bindings for the [Noesis GUI Native SDK](https://www.noesisengine.com/), which brings XAML-driven UI to game engines. You load `.xaml` scenes, drive the View and renderer, implement a `RenderDevice` against your own GPU, and write Rust-backed custom controls and markup extensions that XAML can use by name.

The crate is renderer-agnostic. It's built for Dead Money's own game projects and was mostly written by AI agents under human direction.

## You need a Noesis license

This crate links against the [Noesis Native SDK](https://www.noesisengine.com/), closed-source commercial software from Noesis Technologies S.L. We don't redistribute it. Buy it separately and point `NOESIS_SDK_DIR` at your install; the build script reads it at compile time and links the Noesis library for the matching platform target.

This release targets **Noesis Native SDK 3.2.13**. The C ABI shim is compiled against that version's headers and checks key struct sizes at build time, so a different SDK version may fail to build or link. Match it unless you've verified a newer release.

Apply your license with `noesis_runtime::set_license(name, key)` before `init()`. Without it the runtime works for a while, then blanks the view with a "Trial expired" message.

## Quick start

```toml
[dependencies]
noesis_runtime = "0.13"
```

The crate still links the Noesis SDK at build time, so `NOESIS_SDK_DIR` must be set for it to compile.

```rust
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::{set_xaml_provider, XamlProvider};

// Implement a XAML provider against your asset pipeline.
struct MyXaml(std::collections::HashMap<String, Vec<u8>>);
impl XamlProvider for MyXaml {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

// Once per process.
noesis_runtime::set_license(&license_name, &license_key);
noesis_runtime::init();

// Install the provider. The returned guard owns the registration;
// drop it to clear the global slot.
let provider = MyXaml(/* ... */);
let _guard = set_xaml_provider(provider);

// Load a scene, create a view, and drive it from your frame loop.
let element = FrameworkElement::load("MainMenu.xaml")
    .expect("XAML failed to parse");
let mut view = View::create(element);
view.set_size(1920, 1080);
view.activate();
loop {
    // Forward input to view.mouse_*, view.key_*, view.touch_*
    let _changed = view.update(time_seconds);
    // Render through view.renderer() with your registered RenderDevice.
}

noesis_runtime::shutdown();
```

### Custom controls

```rust
use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyValue,
};
use noesis_runtime::ffi::{ClassBase, PropType};

struct NineSlicerHandler { source_idx: u32, /* ... */ }
impl PropertyChangeHandler for NineSlicerHandler {
    fn on_changed(&self, instance: Instance, idx: u32, _v: PropertyValue<'_>) {
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

// XAML with `xmlns:my="clr-namespace:MyNs"` can now use `<my:NineSlicer/>`.
```

### Custom markup extensions

```rust
use noesis_runtime::markup::MarkupExtensionRegistration;

let table = std::collections::HashMap::from([
    ("menu.greeting", "Hello, world!"),
]);
let _registration = MarkupExtensionRegistration::from_closure(
    "MyNs.Loc",
    move |key| table.get(key).map(|s| s.to_string()),
).expect("markup extension registration failed");

// `{my:Loc menu.greeting}` in XAML now resolves to "Hello, world!".
```

### Render device without the SDK

The default `shim` feature compiles the C++ shim and links Noesis. Turn it off to get only the render-device types (`Batch`, `Tile`, `DeviceCaps`, the shader and state enums) and the `RenderDevice` trait:

```toml
[dependencies]
noesis_runtime = { version = "0.13", default-features = false }
```

That build needs no `NOESIS_SDK_DIR`, compiles no C++, and links no Noesis library. It's for a render device driven by a host that loads Noesis some other way, such as the managed (C#) SDK. Such a host creates the textures, so it maps a batch's texture pointers to handles itself; `register` and the `Batch::*_handle` methods need the shim.

## How it works

- **Hand-written C ABI, no bindgen.** Noesis's C++ API (templates, intrusive `Ptr<T>`, pure-virtual hierarchies) is a poor fit for bindgen, so the binding is a narrow C ABI hand-written in `cpp/noesis_shim.{h,cpp}` and mirrored in `src/ffi.rs`. Noesis types stay opaque on the Rust side and only `#[repr(C)]` POD structs cross the boundary. Struct sizes and enum values are checked against the SDK with compile-time asserts, so an SDK update that reshapes one fails the build instead of producing garbage.
- **RAII registration guards.** Every call that installs something global (`set_xaml_provider`, `subscribe_click`, `ClassBuilder::register`) returns a guard that clears the registration when dropped. Drop every guard and handle before `shutdown()`.
- **Custom classes via trampoline subclasses.** The shim has one C++ subclass per supported base (`RustContentControl`, `RustPanel`, and so on, plus `RustMarkupExtension`). Each reports a synthetic type per registered name and forwards virtuals like `OnPropertyChanged` and `ProvideValue` to your Rust handler.
- **Thread-affine.** Noesis isn't thread-safe. Call into a view, its elements, and its renderer from the one thread that drives it. Handles are `Send` but not `Sync`: you can move them to that thread, not share them across threads.

Custom pixel shaders (`ShaderEffect` / `BrushShader`) aren't wrapped. The render device receives a batch's `pixel_shader` pointer, but there's no way to author one from Rust. For this and the rest of what the SDK can't do, or does differently from WPF, see [SDK limitations](./LIMITATIONS.md).

## Building

```sh
unzip NoesisGUI-NativeSDK-linux-3.2.13-Indie.zip -d ~/sdks/noesis-3.2.13
export NOESIS_SDK_DIR=~/sdks/noesis-3.2.13
cargo test
```

On Windows, point `NOESIS_SDK_DIR` at the unzipped **win** SDK and build with the MSVC toolchain. The build script links `Noesis.lib` from `Lib/windows_x86_64/` and copies `Noesis.dll` from `Bin/windows_x86_64/` next to the test and example binaries, so `cargo test` runs without extra setup. To run a binary from elsewhere, put that `Bin/` directory on `PATH` or keep `Noesis.dll` beside the `.exe`.

Most tests pass `NOESIS_LICENSE_NAME` and `NOESIS_LICENSE_KEY` to `set_license` when both are set:

```sh
export NOESIS_LICENSE_NAME=...
export NOESIS_LICENSE_KEY=...
```

`--features test-utils` compiles the shim's test entry points and enables the tests that need them, including `tests/render_device.rs`, which drives a mock `RenderDevice` through a full frame.

## License

This crate is [MIT](./LICENSE). It contains no Noesis SDK code. Binaries that link the SDK are also subject to the Noesis EULA.

## Acknowledgements

Built on [Noesis Technologies](https://www.noesisengine.com/)' Native SDK. The [Noesis documentation](https://docs.noesisengine.com/) is the source of truth for XAML behavior, control templates, and bindings. Report SDK bugs there; report bugs in this wrapper here.
