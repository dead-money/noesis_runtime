//! `find_name` + `subscribe_click` integration: loads a Button XAML, subscribes
//! a Rust click callback, drives a synthetic click, asserts the callback fires once.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p noesis_runtime --test events -- --nocapture`

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::events::subscribe_click;
use noesis_runtime::view::{FrameworkElement, MouseButton, View};
use noesis_runtime::xaml_provider::XamlProvider;

const BUTTON_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="MyButton" Content="Hi" Width="100" Height="40"
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
fn click_event_fires_callback() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let counter = Arc::new(AtomicU32::new(0));

    {
        // Every owning wrapper must drop before shutdown().
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), BUTTON_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");

        // Tests both paths: pre-view (raw element) and post-view via
        // View::content(). Post-view is the plugin's pattern — View::create
        // consumes the element, so subscriptions must wire after.
        let pre_view = element
            .find_name("MyButton")
            .expect("pre-view find_name returned None");
        assert_eq!(pre_view.name().as_deref(), Some("MyButton"));
        drop(pre_view);

        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");
        let button = content
            .find_name("MyButton")
            .expect("post-view find_name returned None");
        assert_eq!(button.name().as_deref(), Some("MyButton"));

        let counter_in_handler = Arc::clone(&counter);
        let click_sub = subscribe_click(&button, move || {
            counter_in_handler.fetch_add(1, Ordering::SeqCst);
        })
        .expect("subscribe_click returned None — element not a button?");

        let grid_handle = view.content().expect("View::content returned None");
        let grid_subscription = subscribe_click(&grid_handle, || {
            unreachable!("Grid is not a button; subscribe should not have succeeded");
        });
        assert!(
            grid_subscription.is_none(),
            "subscribe_click on Grid unexpectedly succeeded"
        );
        drop(grid_handle);

        // first Update builds the render tree; hit-testing needs it
        assert!(view.update(0.0), "first Update should report change");

        let _ = view.mouse_move(100, 100);
        let _ = view.update(0.016);
        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.032);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.048);

        // drop before view — C++ -= must happen while the button is still alive
        drop(click_sub);
        view.deactivate();
        drop(view);
    }

    noesis_runtime::shutdown();

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "Click handler fired wrong number of times"
    );
}
