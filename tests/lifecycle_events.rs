//! Non-routed lifecycle events (`Event_` / `AddEventHandler` mechanism) — fire and unsubscribe.
//!
//! `LayoutUpdated` and `Initialized` are render-/load-driven and do not re-fire
//! in a headless harness; only their subscription wiring is asserted.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::events::{LifecycleEvent, subscribe_lifecycle, subscribe_lifecycle_by_name};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF101010">
  <Border x:Name="Child" Width="80" Height="40" Background="#FF333333"/>
</Grid>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

#[test]
fn lifecycle_events_fire_and_unsubscribe() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let visible = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");
        let mut child = content.find_name("Child").expect("find Child");

        let visible_h = Arc::clone(&visible);
        let visible_sub =
            subscribe_lifecycle(&child, LifecycleEvent::IsVisibleChanged, move || {
                visible_h.fetch_add(1, Ordering::SeqCst);
            })
            .expect("subscribe IsVisibleChanged returned None");

        let layout_sub = subscribe_lifecycle(&content, LifecycleEvent::LayoutUpdated, || {})
            .expect("subscribe LayoutUpdated returned None");
        let init_sub = subscribe_lifecycle(&content, LifecycleEvent::Initialized, || {})
            .expect("subscribe Initialized returned None");

        assert!(
            subscribe_lifecycle_by_name(&content, "NoSuchLifecycleEvent", || {}).is_none(),
            "unknown lifecycle event name should return None"
        );

        // Pump layout a few times so the tree goes live.
        assert!(view.update(0.0), "first Update should report change");
        let _ = view.update(0.016);
        let _ = view.update(0.032);

        let before = visible.load(Ordering::SeqCst);
        child.set_visibility(false);
        let _ = view.update(0.048);
        child.set_visibility(true);
        let _ = view.update(0.064);
        let after = visible.load(Ordering::SeqCst);
        assert!(
            after > before,
            "IsVisibleChanged should fire when toggling Visibility (before={before}, after={after})"
        );

        // Drop mid-run: continued updates and visibility toggles must not crash.
        drop(visible_sub);
        let at_drop = visible.load(Ordering::SeqCst);
        child.set_visibility(false);
        let _ = view.update(0.080);
        child.set_visibility(true);
        let _ = view.update(0.096);
        assert_eq!(
            visible.load(Ordering::SeqCst),
            at_drop,
            "dropped IsVisibleChanged subscription must stop receiving callbacks"
        );

        drop(layout_sub);
        drop(init_sub);
        drop(child);
        drop(content);
        view.deactivate();
        drop(view);
        drop(_guard);
    }

    noesis_runtime::shutdown();
}
