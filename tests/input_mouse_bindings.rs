//! Mouse gesture bindings fire bound commands end-to-end across the FFI.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noesis_runtime::commands::Command;
use noesis_runtime::input::{InputBinding, ModifierKeys, MouseAction, MouseBinding, MouseGesture};
use noesis_runtime::view::{FrameworkElement, MouseButton, View};
use noesis_runtime::xaml_provider::XamlProvider;

// A button that fully fills the 200x200 view, so a click at its centre (100,100)
// is guaranteed to hit-test onto it.
const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="Target" Content="Hit me"
          HorizontalAlignment="Stretch" VerticalAlignment="Stretch"/>
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
fn mouse_bindings_fire_bound_commands() {
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

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        let target = root.find_name("Target").expect("find Target");

        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&counter);
        let command = Command::new(move |_param| {
            c2.fetch_add(1, Ordering::SeqCst);
        });

        let binding = MouseBinding::new(&command, MouseAction::LeftClick, ModifierKeys::NONE)
            .expect("mouse binding");
        assert!(binding.add_to(&target), "add binding to InputBindings");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "first update builds tree");
        let _ = view.update(0.016);

        let _ = view.mouse_button_down(100, 100, MouseButton::Right);
        let _ = view.update(0.024);
        let _ = view.mouse_button_up(100, 100, MouseButton::Right);
        let _ = view.update(0.032);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "right click must not trigger the LeftClick binding"
        );

        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.04);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.048);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "LeftClick fires the bound command exactly once"
        );

        drop(view);
        drop(binding);
        drop(command);
    }

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        let target = root.find_name("Target").expect("find Target");

        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&counter);
        let command = Command::new(move |_p| {
            c2.fetch_add(1, Ordering::SeqCst);
        });

        // The gesture can be dropped once the binding takes its own reference.
        let binding = {
            let gesture = MouseGesture::new(MouseAction::RightClick, ModifierKeys::NONE);
            InputBinding::with_mouse_gesture(&command, &gesture).expect("input binding")
        };
        assert!(binding.add_to(&target), "attach input binding");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));
        let _ = view.update(0.016);

        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.024);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.032);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "left click must not trigger the RightClick gesture"
        );

        let _ = view.mouse_button_down(100, 100, MouseButton::Right);
        let _ = view.update(0.04);
        let _ = view.mouse_button_up(100, 100, MouseButton::Right);
        let _ = view.update(0.048);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "RightClick gesture fires the bound command exactly once"
        );

        drop(view);
        drop(binding);
        drop(command);
    }

    noesis_runtime::shutdown();
}
