//! `FrameworkElement::mouse_position()`: drive `View::mouse_move` to a known
//! coordinate and verify the element-local position crosses the FFI correctly.

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      x:Name="Root" Background="#FF202020" Width="200" Height="200"/>"##;

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

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn framework_element_mouse_position() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let element = FrameworkElement::load("scene.xaml").expect("scene load");
        // Keep an owning handle before the element is moved into the view.
        let root = element.clone_ref();

        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "first update builds the render tree");

        // Pump a second update after mouse_move so the Mouse subsystem records
        // the position; mouse_position() reads stale without it.
        let _ = view.mouse_move(50, 60);
        let _ = view.update(0.016);

        let pos = root
            .mouse_position()
            .expect("root is a UIElement, so mouse_position resolves");
        assert!(
            approx(pos.0, 50.0) && approx(pos.1, 60.0),
            "mouse position relative to the root grid: expected (50, 60), got {pos:?}"
        );

        view.deactivate();
        drop(view);
        drop(root);
    }

    noesis_runtime::shutdown();
}
