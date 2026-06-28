//! TODO §5 — non-routed lifecycle events (`Event_` mechanism).
//!
//! One headless `#[test]` (Noesis inits once per process). Subscribes Rust
//! callbacks to the non-routed lifecycle events that ride
//! `AddEventHandler(Symbol, EventHandler)` (NOT `AddHandler(RoutedEvent, ...)`)
//! and asserts they actually fire:
//!
//!   * `IsVisibleChanged` — toggling an element's `Visibility` flips `IsVisible`,
//!     which must raise the notification; asserted via a before/after delta on a
//!     fired-callback counter (the meaningful "it actually fired" check).
//!   * `LayoutUpdated` / `Initialized` — subscription wiring must succeed. These
//!     are render-/load-driven and do not fire again in this headless harness
//!     (same as `Loaded`), so only the subscribe path is asserted for them.
//!
//! Plus the contract edges: an unknown event name returns `None`, and dropping a
//! subscription mid-run unsubscribes cleanly (no crash on continued updates or
//! shutdown).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::events::{LifecycleEvent, subscribe_lifecycle, subscribe_lifecycle_by_name};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let visible = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");
        let mut child = content.find_name("Child").expect("find Child");

        // IsVisibleChanged on the child — fires when IsVisible flips.
        let visible_h = Arc::clone(&visible);
        let visible_sub =
            subscribe_lifecycle(&child, LifecycleEvent::IsVisibleChanged, move || {
                visible_h.fetch_add(1, Ordering::SeqCst);
            })
            .expect("subscribe IsVisibleChanged returned None");

        // LayoutUpdated / Initialized — wiring must succeed (render-/load-driven;
        // they don't fire again in this headless harness, like Loaded).
        let layout_sub = subscribe_lifecycle(&content, LifecycleEvent::LayoutUpdated, || {})
            .expect("subscribe LayoutUpdated returned None");
        let init_sub = subscribe_lifecycle(&content, LifecycleEvent::Initialized, || {})
            .expect("subscribe Initialized returned None");

        // Negative: unknown event name must not subscribe.
        assert!(
            subscribe_lifecycle_by_name(&content, "NoSuchLifecycleEvent", || {}).is_none(),
            "unknown lifecycle event name should return None"
        );

        // Pump layout a few times so the tree goes live.
        assert!(view.update(0.0), "first Update should report change");
        let _ = view.update(0.016);
        let _ = view.update(0.032);

        // Toggle visibility and confirm IsVisibleChanged observes the change.
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

        // Drop the IsVisibleChanged subscription mid-run; continued updates +
        // toggles must not crash and must not increment its counter further.
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

    dm_noesis_runtime::shutdown();
}
