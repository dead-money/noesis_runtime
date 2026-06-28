//! TODO §16 — input gestures + bindings, end-to-end across the FFI.
//!
//! The strong wiring proof: a Rust [`Command`] whose `Execute` bumps an
//! `Arc<AtomicUsize>` is bound to a `Ctrl+Enter` [`KeyBinding`], added to a
//! focused element's `InputBindings`. Driving `Ctrl+Enter` through a live `View`
//! must run the command — proving gesture → binding → command across the
//! boundary. A second case does the same with an explicit [`KeyGesture`] +
//! [`InputBinding`] to exercise the general gesture path.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   cargo test --test `input_bindings` -- --nocapture

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noesis_runtime::commands::Command;
use noesis_runtime::input::{InputBinding, KeyBinding, KeyGesture, ModifierKeys};
use noesis_runtime::view::{FrameworkElement, Key, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="Target" Content="Hit me" Width="160" Height="40"
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

// One #[test] per file (init once per process — the headless convention). Both
// the KeyBinding and the explicit-gesture InputBinding cases run sequentially.
#[test]
fn input_bindings_fire_bound_commands() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    // ── Case 1: Ctrl+Enter KeyBinding ───────────────────────────────────────
    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        let mut target = root.find_name("Target").expect("find Target");

        // A Rust command that bumps a shared counter on Execute.
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&counter);
        let command = Command::new(move |_param| {
            c2.fetch_add(1, Ordering::SeqCst);
        });

        // Ctrl+Enter → command, attached to the target element.
        let binding =
            KeyBinding::new(&command, Key::Return, ModifierKeys::CONTROL).expect("key binding");
        assert!(binding.add_to(&target), "add binding to InputBindings");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "first update builds tree");

        assert!(target.focus(), "Button accepts focus");
        let _ = view.update(0.016);
        assert!(target.is_keyboard_focused(), "target focused");

        // Sanity: a bare Enter (no Ctrl) must NOT match the Ctrl+Enter gesture.
        let _ = view.key_down(Key::Return);
        let _ = view.update(0.024);
        let _ = view.key_up(Key::Return);
        let _ = view.update(0.032);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "plain Enter must not trigger Ctrl+Enter binding"
        );

        // Now Ctrl+Enter: press Control (sets the modifier), then Enter.
        let _ = view.key_down(Key::LeftCtrl);
        let _ = view.update(0.04);
        let _ = view.key_down(Key::Return);
        let _ = view.update(0.048);
        let _ = view.key_up(Key::Return);
        let _ = view.key_up(Key::LeftCtrl);
        let _ = view.update(0.056);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "Ctrl+Enter must fire the bound command exactly once"
        );

        drop(view);
        drop(binding);
        drop(command);
    }

    // ── Case 2: explicit KeyGesture wrapped in a generic InputBinding ────────
    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        let mut target = root.find_name("Target").expect("find Target");

        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&counter);
        let command = Command::new(move |_p| {
            c2.fetch_add(1, Ordering::SeqCst);
        });

        // Build a standalone gesture (F5, no modifiers) and wrap it in a generic
        // InputBinding — the gesture object can be dropped once the binding adds
        // its own reference.
        let binding = {
            let gesture = KeyGesture::new(Key::F5, ModifierKeys::NONE);
            InputBinding::with_gesture(&command, &gesture).expect("input binding")
        };
        assert!(binding.add_to(&target), "attach input binding");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));
        assert!(target.focus());
        let _ = view.update(0.016);
        assert!(target.is_keyboard_focused());

        let _ = view.key_down(Key::F5);
        let _ = view.update(0.024);
        let _ = view.key_up(Key::F5);
        let _ = view.update(0.032);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "F5 gesture fires the bound command"
        );

        drop(view);
        drop(binding);
        drop(command);
    }

    noesis_runtime::shutdown();
}
