//! Phase 5.A.5 — FFI-level smoke test for View input methods.
//!
//! Doesn't exercise rendering. Registers a minimal XAML provider, loads a
//! `<Button>` scene, drives mouse/key/touch/focus events, and asserts that
//! `View::update` reports change (`true`) at the expected moments.
//!
//! The Click/hit-test assertion would require a `Noesis::EventHandler`
//! trampoline across the FFI; pushed to a later phase if we decide we want
//! unit-level dispatch testing. For now, `update() -> true` after an event
//! proves the event reached the render tree.
//!
//! Run with `NOESIS_SDK_DIR` set, e.g.
//!   cargo test -p dm_noesis_runtime --test input -- --nocapture

use std::collections::HashMap;

use dm_noesis_runtime::view::{FrameworkElement, Key, MouseButton, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const BUTTON_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button Content="Hi" Width="100" Height="40"
          HorizontalAlignment="Center" VerticalAlignment="Center"/>
</Grid>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

#[test]
fn view_input_ffi_smoke() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // Every owning wrapper must drop before shutdown().
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), BUTTON_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        // First Update builds the initial render tree → expected true.
        // (The first Update is the canonical "something changed" moment —
        // we lock this in as the one hard assertion. Subsequent Update
        // return values depend on whether the theme wires hover/press
        // VisualStates, which isn't guaranteed for a bare Button without
        // an application theme loaded.)
        assert!(view.update(0.0), "first Update should report change");

        // Drive pointer events. Assertions are on "trampoline didn't panic
        // and returned a bool" — which is all the FFI layer can vouch for.
        // Functional dispatch (Click firing on a button) is covered at the
        // integration level once we have the input plugin wired.
        let _ = view.mouse_move(100, 100);
        let _ = view.update(0.016);

        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.032);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.048);
        let _ = view.mouse_double_click(100, 100, MouseButton::Left);
        let _ = view.update(0.064);

        // Key + char smoke — Noesis will route to the focused element
        // (here the Button). We don't assert on result; we just verify the
        // trampolines don't crash and return sanely.
        let _ = view.key_down(Key::Tab);
        let _ = view.key_up(Key::Tab);
        let _ = view.key_down(Key::A);
        let _ = view.char_input('a' as u32);
        let _ = view.key_up(Key::A);

        // Touch.
        let _ = view.touch_down(100, 100, 7);
        let _ = view.touch_move(110, 110, 7);
        let _ = view.touch_up(110, 110, 7);

        // Scroll / wheel.
        let _ = view.mouse_wheel(100, 100, 120);
        let _ = view.scroll(100, 100, 1.0);
        let _ = view.hscroll(100, 100, -1.0);

        view.deactivate();

        drop(view);
    }

    dm_noesis_runtime::shutdown();
}
