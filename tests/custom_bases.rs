//! Phase C — custom base classes (TODO §9 "More base classes").
//!
//! Registers a Rust-backed subclass for EACH new base (`Control`,
//! `FrameworkElement`, `UserControl`, `Panel`, `Decorator`), and asserts:
//!   * every registration succeeds (the synthetic `TypeClass` + Factory creator
//!     install for each base),
//!   * a DP round-trips through Noesis DP storage on a code-created instance of
//!     each base,
//!   * a non-`ContentControl` base (`Control`) resolves from XAML, participates
//!     in layout (`Width="120"` -> `ActualWidth` 120 read back through Noesis),
//!     and fires the property-changed callback for a DP set in XAML.

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::{FrameworkElement, View};

#[derive(Clone, Default)]
struct Recorder {
    inner: Arc<Mutex<Vec<(u32, i32)>>>,
}
struct Handler {
    rec: Recorder,
}
impl PropertyChangeHandler for Handler {
    fn on_changed(&self, _instance: Instance, prop_index: u32, value: PropertyValue<'_>) {
        if let PropertyValue::Int32(v) = value {
            self.rec.inner.lock().unwrap().push((prop_index, v));
        }
    }
}

const CONTROL_XAML: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:b="clr-namespace:Bases" Width="400" Height="300">
  <b:MyControl x:Name="C" Width="120" Height="40" Counter="7"
               HorizontalAlignment="Left" VerticalAlignment="Top"/>
</Grid>"##;

#[test]
fn custom_base_classes() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // One code-created instance per base; round-trip an Int32 DP through
        // Noesis DP storage (read back via Noesis, not a Rust cache).
        let bases = [
            ("Bases.FE", ClassBase::FrameworkElement, 11),
            ("Bases.UC", ClassBase::UserControl, 22),
            ("Bases.PN", ClassBase::Panel, 33),
            ("Bases.DC", ClassBase::Decorator, 44),
        ];
        let mut regs = Vec::new();
        for (name, base, val) in bases {
            let mut b = ClassBuilder::new(
                name,
                base,
                Handler {
                    rec: Recorder::default(),
                },
            );
            let idx = b.add_property("Value", PropType::Int32);
            assert_eq!(idx, 0);
            let reg = b
                .register()
                .unwrap_or_else(|| panic!("registration failed for {name}"));
            let inst = reg.create_instance().expect("create_instance");
            let h = inst.handle();
            h.set_int32(0, val);
            assert_eq!(
                h.get_int32(0),
                Some(val),
                "DP round-trip failed for base {base:?}"
            );
            regs.push((reg, inst));
        }

        // Control base from XAML: resolve, lay out, and fire the change cb.
        let recorder = Recorder::default();
        let mut cb = ClassBuilder::new(
            "Bases.MyControl",
            ClassBase::Control,
            Handler {
                rec: recorder.clone(),
            },
        );
        let counter_idx = cb.add_property("Counter", PropType::Int32);
        let control_reg = cb.register().expect("MyControl registration failed");

        let root = FrameworkElement::parse(CONTROL_XAML).expect("parse CONTROL_XAML");
        let mut view = View::create(root);
        view.set_size(400, 300);
        view.activate();
        assert!(view.update(0.0));

        let content = view.content().expect("content");
        let c = content.find_name("C").expect("find C");
        // ActualWidth read back THROUGH Noesis after layout.
        assert_eq!(
            c.actual_width(),
            Some(120.0),
            "custom Control did not participate in layout"
        );

        let recorded = recorder.inner.lock().unwrap().clone();
        assert!(
            recorded.iter().any(|(i, v)| *i == counter_idx && *v == 7),
            "expected Counter=7 change from XAML; got {recorded:?}"
        );

        drop(c);
        drop(content);
        view.deactivate();
        drop(view);
        drop(control_reg);
        // Drop instances before their registrations.
        for (reg, inst) in regs.into_iter().rev() {
            drop(inst);
            drop(reg);
        }
    }

    dm_noesis_runtime::shutdown();
}
