//! TODO §9 (B) — custom routed events on a Rust-backed type.
//!
//! Registers a routed event on a Rust-backed `ContentControl` type, subscribes a
//! Rust handler via the generic `subscribe_event` surface, raises the event from
//! Rust, and asserts the handler fired exactly once. Negative cases: an
//! unregistered event name cannot be subscribed/raised, and after the
//! subscription drops, raising fires nothing. A stubbed registration makes the
//! event unresolvable, so subscribe / raise return false and the counter stays 0.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::events::subscribe_event;
use dm_noesis_runtime::ffi::ClassBase;
use dm_noesis_runtime::reflection::{RoutingStrategy, raise_event, register_routed_event};
use dm_noesis_runtime::view::FrameworkElement;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:dm="clr-namespace:DmTest">
  <dm:Eventful x:Name="Target"/>
</Grid>"##;

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&mut self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

#[test]
fn custom_routed_event_fires() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let counter = Arc::new(AtomicU32::new(0));

    {
        // Register the Rust-backed type, then a routed event on it, BEFORE the
        // XAML that references it is parsed.
        let _reg = ClassBuilder::new("DmTest.Eventful", ClassBase::ContentControl, NoopHandler)
            .register()
            .expect("class registration failed");

        assert!(
            register_routed_event("DmTest.Eventful", "MyEvent", RoutingStrategy::Bubble),
            "register_routed_event returned false"
        );
        // Duplicate registration on the same type must be rejected.
        assert!(
            !register_routed_event("DmTest.Eventful", "MyEvent", RoutingStrategy::Bubble),
            "duplicate routed-event registration should be rejected"
        );
        // Registering on an unknown type must fail.
        assert!(
            !register_routed_event("DmTest.NoSuchType", "MyEvent", RoutingStrategy::Bubble),
            "routed event on unknown type should be rejected"
        );

        let root = FrameworkElement::parse(XAML).expect("parse returned None");
        let target = root
            .find_name("Target")
            .expect("find_name(Target) returned None");

        // Subscribing to a never-registered event must fail.
        assert!(
            subscribe_event(&target, "NotAnEvent", false, |_: &_| false).is_none(),
            "subscribing to an unregistered event should return None"
        );

        let c = Arc::clone(&counter);
        let sub = subscribe_event(&target, "MyEvent", false, move |_args: &_| {
            c.fetch_add(1, Ordering::SeqCst);
            false
        })
        .expect("subscribe_event(MyEvent) returned None");

        // Raising an unregistered event is a no-op (false, no fire).
        assert!(
            !raise_event(&target, "NotAnEvent"),
            "raising an unregistered event should return false"
        );
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Raise the real event — the handler must fire exactly once.
        assert!(
            raise_event(&target, "MyEvent"),
            "raise_event returned false"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "handler should have fired exactly once"
        );

        // After unsubscribing, raising fires nothing more.
        drop(sub);
        assert!(
            raise_event(&target, "MyEvent"),
            "raise_event returned false"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "no handler should fire after unsubscribe"
        );
    }

    dm_noesis_runtime::shutdown();
}
