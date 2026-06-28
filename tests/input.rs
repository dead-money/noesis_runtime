//! FFI-level smoke test for View input methods (mouse, key, touch, scroll).
//! Asserts `View::update` reports change after the first update; does not test rendering.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p noesis_runtime --test input -- --nocapture`

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, Key, MouseButton, View};
use noesis_runtime::xaml_provider::XamlProvider;

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
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // Every owning wrapper must drop before shutdown().
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), BUTTON_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        // First Update builds the render tree — always reports change.
        // Subsequent Updates depend on theme VisualStates (not guaranteed headless).
        assert!(view.update(0.0), "first Update should report change");

        let _ = view.mouse_move(100, 100);
        let _ = view.update(0.016);

        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.032);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.048);
        let _ = view.mouse_double_click(100, 100, MouseButton::Left);
        let _ = view.update(0.064);

        let _ = view.key_down(Key::Tab);
        let _ = view.key_up(Key::Tab);
        let _ = view.key_down(Key::A);
        let _ = view.char_input('a' as u32);
        let _ = view.key_up(Key::A);

        let _ = view.touch_down(100, 100, 7);
        let _ = view.touch_move(110, 110, 7);
        let _ = view.touch_up(110, 110, 7);

        let _ = view.mouse_wheel(100, 100, 120);
        let _ = view.scroll(100, 100, 1.0);
        let _ = view.hscroll(100, 100, -1.0);

        view.deactivate();

        drop(view);
    }

    noesis_runtime::shutdown();
}
