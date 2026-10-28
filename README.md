# noesis_runtime

[![CI](https://github.com/dead-money/noesis_runtime/actions/workflows/ci.yml/badge.svg)](https://github.com/dead-money/noesis_runtime/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/noesis_runtime.svg)](https://crates.io/crates/noesis_runtime)
[![docs.rs](https://docs.rs/noesis_runtime/badge.svg)](https://docs.rs/noesis_runtime)

Rust bindings for the [Noesis GUI Native SDK](https://www.noesisengine.com/), which brings XAML-driven UI to game engines. You load `.xaml` scenes, drive the View and renderer, implement a `RenderDevice` against your own GPU, and write Rust-backed custom controls and markup extensions that XAML can use by name.

The crate is renderer-agnostic.

## You need a Noesis license

This crate links against the [Noesis Native SDK](https://www.noesisengine.com/), closed-source commercial software from Noesis Technologies S.L. We don't redistribute it. Buy it separately and point `NOESIS_SDK_DIR` at your install; the build script reads it at compile time and links the Noesis library for the matching platform target.

This release targets **Noesis Native SDK 4.0.1 (r18228)**. The build checks the SDK version and the render-device ABI before compiling the shim.

Apply your license with `noesis_runtime::set_license(name, key)` before `init()`. Without it the runtime works for a while, then blanks the view with a "Trial expired" message.

## Version compatibility

| noesis_runtime | Noesis Native SDK |
|----------------|-------------------|
| 0.14 | 4.0.1 (r18228) |
| 0.13 | 3.2.13 (r17073) |

The SDK version applies to both the native shim and the render-device types
exposed without `shim`. Use a matching crate line when another host loads Noesis.

## Quick start

```toml
[dependencies]
noesis_runtime = "0.14"
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
noesis_runtime = { version = "0.14", default-features = false }
```

That build needs no `NOESIS_SDK_DIR`, compiles no C++, and links no Noesis library. It's for a render device driven by a host that loads Noesis some other way, such as the managed (C#) SDK. Such a host creates the textures, so it maps a batch's texture pointers to handles itself; `register` and the `Batch::*_handle` methods need the shim.

## Ownership and threads

Keep registration guards alive while their callbacks are needed, and drop all
guards and Noesis handles before `shutdown()`. Drive a view, its elements and
its renderer from one thread. Handles are `Send` but not `Sync`.

See [SDK limitations](./LIMITATIONS.md) for unsupported APIs and differences
from WPF.

## Building

The 4.0.1 download retains `4.0.0` in its archive filename.

```sh
unzip NoesisGUI-NativeSDK-linux-4.0.0-Indie.zip -d ~/sdks/noesis-4.0.1
export NOESIS_SDK_DIR=~/sdks/noesis-4.0.1
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
