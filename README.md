# dm_noesis_runtime

Rust bindings for the [Noesis GUI Native SDK](https://www.noesisengine.com/), which brings XAML-driven UI to game engines. You load `.xaml` scenes, drive the View and renderer, implement a `RenderDevice` against your own GPU, and write Rust-backed custom controls and markup extensions that XAML can use by name.

The crate is renderer-agnostic; Bevy 0.18 integration lives in the sibling crate [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy). It's built for Dead Money's own game projects and was mostly written by AI agents under human direction, so expect interfaces to change.

## You need a Noesis license

This crate links against the [Noesis Native SDK](https://www.noesisengine.com/), closed-source commercial software from Noesis Technologies S.L. We don't redistribute it. Buy it separately and point `NOESIS_SDK_DIR` at your install; the build script reads it at compile time and links `libNoesis` from the matching `Bin/<platform>/` directory.

Set `NOESIS_LICENSE_NAME` and `NOESIS_LICENSE_KEY` to apply your license. Without them the runtime works for a while, then blanks the view with a "Trial expired" message.

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

// Once per process.
dm_noesis_runtime::init();

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
    // Your renderer drives view.renderer().
    // See dm_noesis_bevy for a complete integration.
}

dm_noesis_runtime::shutdown();
```

For the full pipeline (XAML, font, and texture providers, the render device, and input) wired against Bevy 0.18, see [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy).

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

// XAML with `xmlns:my="clr-namespace:MyNs"` can now use `<my:NineSlicer/>`.
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

## How it works

- **Hand-written C ABI, no bindgen.** Noesis's C++ API (templates, intrusive `Ptr<T>`, pure-virtual hierarchies) is a poor fit for bindgen, so the binding is a narrow C ABI hand-written in `cpp/noesis_shim.{h,cpp}` and mirrored in `src/ffi.rs`. Noesis types stay opaque on the Rust side; only `#[repr(C)]` POD structs cross the boundary, and each one's size is checked at compile time, so an SDK update that reshapes a struct fails the build instead of producing garbage.
- **RAII registration guards.** Every "install something global" call (`set_xaml_provider`, `subscribe_click`, `register_class`) hands back a guard that clears the registration when dropped. Drop order matters; the per-module docs spell out the contract.
- **Custom controls via trampoline subclasses.** The shim's `RustContentControl` and `RustMarkupExtension` report a synthetic `TypeClass` per name and forward virtuals like `OnPropertyChanged` and `ProvideValue` to your Rust callback, the same shape Noesis's own C# and Unity bindings use.
- **Single-threaded.** Noesis isn't thread-safe. The view, renderer, and input pump all run on one rendering thread. The guards are `Send`, so you can move resources between threads, but the calls themselves stay on that thread.

Custom pixel shaders (`BrushShader` / `ShaderEffect`) are out of scope. They need compiled shader bytecode and a live render device to do anything, which the crate's headless FFI surface can't provide. The `Batch::pixel_shader` pointer round-trips through the render device, but there's no way to author one here.

For the rest of what the SDK can't do, or does differently from WPF, see [SDK limitations](./LIMITATIONS.md).

## Building

```sh
unzip NoesisGUI-NativeSDK-linux-3.2.13-Indie.zip -d ~/sdks/noesis-3.2.13
export NOESIS_SDK_DIR=~/sdks/noesis-3.2.13
cargo test
```

Optionally apply your license credentials (see above):

```sh
export NOESIS_LICENSE_NAME=...
export NOESIS_LICENSE_KEY=...
```

`tests/lifecycle.rs` calls `init`, `version`, and `shutdown` and checks for a non-empty version string. Building with `--features test-utils` unlocks `tests/render_device.rs`, the full render device regression test.

## Licensing

Source in this repository is © 2026 Dead Money LLC and is distributed under the [MIT License](./LICENSE). Everything under `cpp/`, `src/`, and `tests/` is original work. No Noesis SDK code is vendored or translated; it's only `#include`'d at compile time from `NOESIS_SDK_DIR`.

The Noesis Native SDK is not part of this repository and is not redistributed here. You obtain it from Noesis Technologies under their EULA. Use and distribution of any binary you build that links against the SDK is governed by that EULA, not by the MIT License above.

## Acknowledgements

Built on [Noesis Technologies](https://www.noesisengine.com/)' Native SDK. The [Noesis documentation](https://docs.noesisengine.com/) is the source of truth for XAML behavior, control templates, and bindings. Report SDK bugs there; report bugs in this wrapper here.
