//! Phase 6 — `MultiDataTrigger` (binding-condition sibling of `MultiTrigger`)
//! and `EventTrigger` action attachment (`BeginStoryboard`).
//!
//! Both build from code and read back out of the LIVE Noesis objects:
//!   * `MultiDataTrigger` conditions carry a `Binding` + boxed `Value` (re-read
//!     via `condition_has_binding` / `condition_value`); setters are counted. The
//!     trigger is also attached to a `Style` to prove it is a real `BaseTrigger`.
//!   * `EventTrigger` gains a `BeginStoryboard` action, observed through the live
//!     `action_count` (which was previously read-only with no way to grow it).
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test multi_data_trigger -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::animation::{BeginStoryboard, Storyboard};
use dm_noesis_runtime::binding::{Binding, box_bool, box_f32, box_string};
use dm_noesis_runtime::styles::{EventTrigger, MultiDataTrigger, Style};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// A scene referencing the control types resolved by name, so the reflection
// registry knows them (the built-ins register on first use).
const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="200">
  <StackPanel>
    <TextBlock Text="A"/>
    <Button Content="B"/>
  </StackPanel>
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

fn register_types() -> View {
    let mut bytes = HashMap::new();
    bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
    let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });
    let root = FrameworkElement::load("scene.xaml").expect("scene load");
    let mut view = View::create(root);
    view.set_size(200, 200);
    view.activate();
    view.update(0.0);
    view
}

#[test]
fn multi_data_trigger_and_event_actions_roundtrip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let _view = register_types();

        // ── MultiDataTrigger ────────────────────────────────────────────────
        let mut mdt = MultiDataTrigger::new();
        assert!(
            mdt.add_condition(&Binding::new("IsEnabled"), &box_bool(true)),
            "binding condition #0"
        );
        assert!(
            mdt.add_condition(&Binding::new("Tag"), &box_string("ready")),
            "binding condition #1"
        );
        assert_eq!(mdt.condition_count(), 2, "two conditions");
        assert!(mdt.condition_has_binding(0), "condition[0] has a Binding");
        assert!(mdt.condition_has_binding(1), "condition[1] has a Binding");
        assert!(
            !mdt.condition_has_binding(7),
            "out-of-range condition reports no binding"
        );
        assert_eq!(
            mdt.condition_value(0).and_then(|v| v.as_bool()),
            Some(true),
            "condition[0] Value round-trip"
        );
        assert_eq!(
            mdt.condition_value(1)
                .and_then(|v| v.as_string())
                .as_deref(),
            Some("ready"),
            "condition[1] Value round-trip"
        );
        assert!(
            mdt.add_setter("TextBlock", "FontSize", &box_f32(18.0)),
            "setter resolves on TextBlock"
        );
        assert!(
            !mdt.add_setter("TextBlock", "Nope", &box_f32(1.0)),
            "unknown DP must not add a setter"
        );
        assert_eq!(mdt.setter_count(), 1, "one setter");

        // It's a real BaseTrigger — attach it to a Style.
        let mut style = Style::new();
        assert!(style.set_target_type("TextBlock"));
        assert!(
            style.add_trigger(&mdt),
            "MultiDataTrigger attaches to a Style"
        );
        assert_eq!(style.trigger_count(), 1, "Style.Triggers holds it");

        // ── EventTrigger action attachment ──────────────────────────────────
        let mut et = EventTrigger::new();
        assert!(et.set_routed_event("Button", "Click"), "Click resolves");
        assert_eq!(et.action_count(), 0, "no actions yet");

        let mut bs = BeginStoryboard::new();
        let sb = Storyboard::new();
        assert!(
            bs.set_storyboard(&sb),
            "assign storyboard to BeginStoryboard"
        );
        assert!(et.add_action(&bs), "attach BeginStoryboard action");
        assert_eq!(
            et.action_count(),
            1,
            "Actions collection grew (read back from the live object)"
        );

        let bs2 = BeginStoryboard::new();
        assert!(et.add_action(&bs2), "attach a second action");
        assert_eq!(et.action_count(), 2, "two actions");
    }

    dm_noesis_runtime::shutdown();
}
