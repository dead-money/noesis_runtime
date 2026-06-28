//! Custom dependency-property metadata: coercion (Float clamped to [0, 100]),
//! read-only DPs (ordinary setter rejected; privileged key-path setter writes),
//! and `AffectsMeasure` accepted at registration.

use std::sync::{Arc, Mutex};

use noesis_runtime::classes::{
    ClassBuilder, CoerceHandler, Coerced, Instance, PropertyChangeHandler, PropertyDefault,
    PropertyOptions, PropertyValue, fpm_options,
};
use noesis_runtime::ffi::{ClassBase, PropType};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[derive(Clone, Default)]
struct CoerceLog {
    calls: Arc<Mutex<u32>>,
}
struct Clamp {
    log: CoerceLog,
}
impl CoerceHandler for Clamp {
    fn coerce(&self, _instance: Instance, prop_index: u32, value: PropertyValue<'_>) -> Coerced {
        *self.log.calls.lock().unwrap() += 1;
        // Only the "Clamped" property (index 0) is routed through coercion.
        if prop_index == 0
            && let PropertyValue::Float(v) = value
        {
            return Coerced::Float(v.clamp(0.0, 100.0));
        }
        Coerced::Unchanged
    }
}

#[test]
fn custom_dp_metadata() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let log = CoerceLog::default();

        let mut b = ClassBuilder::new("Meta.Thing", ClassBase::FrameworkElement, NoopChange);
        let clamped = b.add_property_ex(
            "Clamped",
            PropType::Float,
            PropertyDefault::Float(50.0),
            PropertyOptions {
                coerce: true,
                ..Default::default()
            },
        );
        let ro = b.add_property_ex(
            "ReadOnlyVal",
            PropType::Int32,
            PropertyDefault::Int32(0),
            PropertyOptions {
                read_only: true,
                ..Default::default()
            },
        );
        let measured = b.add_property_ex(
            "Measured",
            PropType::Float,
            PropertyDefault::Float(0.0),
            PropertyOptions {
                fpm_options: fpm_options::AFFECTS_MEASURE | fpm_options::AFFECTS_RENDER,
                ..Default::default()
            },
        );
        assert_eq!((clamped, ro, measured), (0, 1, 2));
        b.set_coerce(Clamp { log: log.clone() });

        let reg = b.register().expect("registration failed");
        assert_eq!(reg.num_properties(), 3);

        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        // COERCION: read back through Noesis.
        h.set_float(clamped, 999.0);
        assert_eq!(h.get_float(clamped), Some(100.0), "coerce upper clamp");
        h.set_float(clamped, -5.0);
        assert_eq!(h.get_float(clamped), Some(0.0), "coerce lower clamp");
        h.set_float(clamped, 42.0);
        assert_eq!(h.get_float(clamped), Some(42.0), "coerce passthrough");
        assert!(*log.calls.lock().unwrap() >= 3, "coerce handler never ran");

        // READ-ONLY: ordinary setter is a no-op; key-path setter works.
        h.set_int32(ro, 5); // ordinary path: rejected
        assert_eq!(
            h.get_int32(ro),
            Some(0),
            "ordinary setter must not write a read-only DP"
        );
        assert!(
            h.set_readonly_int32(ro, 5),
            "privileged setter returned false"
        );
        assert_eq!(
            h.get_int32(ro),
            Some(5),
            "read-only DP did not update via key path"
        );

        h.set_float(measured, 7.5);
        assert_eq!(h.get_float(measured), Some(7.5));

        drop(inst);
        drop(reg);
    }

    noesis_runtime::shutdown();
}
