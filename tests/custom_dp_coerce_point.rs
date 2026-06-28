//! TODO §9 — COERCION on a NEW DP value type (`Point`).
//!
//! Mirrors the scalar coerce test in `tests/custom_dp_metadata.rs`, but on a
//! `Point` DP: a handler clamps both coordinates to `>= 0` and returns
//! `Coerced::Point`. A no-op coerce would leave the negative input intact, so
//! clamping discriminates a stubbed trampoline and proves the new `Coerced::Point`
//! encode arm + the C++ `coercible_size`/`create_dp_ex` Point path round-trip.
//! (Read back THROUGH Noesis on a code-created instance.)

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::classes::{
    ClassBuilder, CoerceHandler, Coerced, Instance, PropertyChangeHandler, PropertyDefault,
    PropertyOptions, PropertyValue,
};
use dm_noesis_runtime::ffi::{ClassBase, PropType};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[derive(Clone, Default)]
struct CoerceLog {
    calls: Arc<Mutex<u32>>,
}

struct ClampPoint {
    log: CoerceLog,
}
impl CoerceHandler for ClampPoint {
    fn coerce(&self, _instance: Instance, prop_index: u32, value: PropertyValue<'_>) -> Coerced {
        *self.log.calls.lock().unwrap() += 1;
        if prop_index == 0
            && let PropertyValue::Point { x, y } = value
        {
            return Coerced::Point {
                x: x.max(0.0),
                y: y.max(0.0),
            };
        }
        Coerced::Unchanged
    }
}

#[test]
fn custom_dp_coerce_point() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();
    {
        let log = CoerceLog::default();

        let mut b = ClassBuilder::new("Meta.PointThing", ClassBase::FrameworkElement, NoopChange);
        let pt = b.add_property_ex(
            "ClampedPt",
            PropType::Point,
            PropertyDefault::Point { x: 0.0, y: 0.0 },
            PropertyOptions {
                coerce: true,
                ..Default::default()
            },
        );
        assert_eq!(pt, 0);
        b.set_coerce(ClampPoint { log: log.clone() });

        let reg = b.register().expect("registration failed");
        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        // Negative coords are clamped to zero — read back through Noesis.
        h.set_point(pt, -3.0, 7.0);
        assert_eq!(h.get_point(pt), Some((0.0, 7.0)), "Point coerce clamp x");
        h.set_point(pt, 4.0, -2.0);
        assert_eq!(h.get_point(pt), Some((4.0, 0.0)), "Point coerce clamp y");
        // A fully-positive value passes through unchanged.
        h.set_point(pt, 5.0, 6.0);
        assert_eq!(
            h.get_point(pt),
            Some((5.0, 6.0)),
            "Point coerce passthrough"
        );
        assert!(
            *log.calls.lock().unwrap() >= 3,
            "Point coerce handler never ran"
        );

        drop(inst);
        drop(reg);
    }
    dm_noesis_runtime::shutdown();
}
