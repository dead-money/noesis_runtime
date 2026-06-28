//! Phase 3 regression — re-entrant property-change handler.
//!
//! The crate's documented "computed property" pattern has a
//! [`PropertyChangeHandler`] write *another* DP synchronously from inside
//! `on_changed`. That write re-enters Noesis, which re-invokes the *same*
//! handler box before the outer call returns. The trampoline must therefore
//! never hold a `&mut` to the handler across the user callback (that would alias
//! on re-entry — undefined behaviour). Here `on_changed` for the `In` DP writes
//! the `Out` DP on the same instance; we assert the re-entrant callback runs to
//! completion and both values land.
//!
//! Like the other custom-DP callback tests, the change callback only fires on a
//! parsed / tree-attached instance, so we register the class, load it in a live
//! `View`, settle the tree, then mutate the input DP through the parsed
//! instance.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:nz="clr-namespace:NzRe"
      Width="100" Height="100">
  <nz:Widget x:Name="W"/>
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

/// Handler uses interior mutability (`&self` callback) and writes the `Out` DP
/// from inside the `In` DP's change notification — the re-entrant path.
struct Computed {
    in_idx: u32,
    out_idx: u32,
    log: Arc<Mutex<Vec<(u32, f32)>>>,
}

impl PropertyChangeHandler for Computed {
    fn on_changed(&self, instance: Instance, prop_index: u32, value: PropertyValue<'_>) {
        if let PropertyValue::Float(f) = value {
            self.log.lock().unwrap().push((prop_index, f));
            if prop_index == self.in_idx {
                // Synchronous DP write → re-enters this same handler box for
                // `out_idx`. The `out_idx` branch does nothing, so no infinite
                // loop. Under `&mut` this would be aliasing UB.
                instance.set_float(self.out_idx, f * 2.0);
            }
        }
    }
}

#[test]
fn custom_dp_reentrant_change() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let log = Arc::new(Mutex::new(Vec::<(u32, f32)>::new()));
    {
        let mut b = ClassBuilder::new(
            "NzRe.Widget",
            ClassBase::FrameworkElement,
            Computed {
                in_idx: 0,
                out_idx: 1,
                log: Arc::clone(&log),
            },
        );
        let in_idx = b.add_property("In", PropType::Float);
        let out_idx = b.add_property("Out", PropType::Float);
        assert_eq!((in_idx, out_idx), (0, 1));
        let reg = b.register().expect("class registration failed");

        let mut bytes = HashMap::new();
        bytes.insert("re.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("re.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(100, 100);
        view.activate();
        assert!(view.update(0.0));

        let content = view.content().expect("View::content returned None");
        let widget = content.find_name("W").expect("find_name(W) returned None");
        let inst = unsafe { Instance::from_raw(std::ptr::NonNull::new(widget.raw()).unwrap()) };

        // Drive the input DP. The change callback fires on the tree-attached
        // instance, and its body re-enters the trampoline via set_float(out).
        log.lock().unwrap().clear();
        inst.set_float(in_idx, 21.0);

        // Both values landed: the input we wrote, and the computed output the
        // re-entrant callback wrote.
        assert_eq!(inst.get_float(in_idx), Some(21.0), "input DP did not land");
        assert_eq!(
            inst.get_float(out_idx),
            Some(42.0),
            "re-entrant callback did not write the output DP"
        );

        // The log proves the re-entrant callback actually executed: we observed
        // a change for `In` (21.0) and, nested inside it, a change for `Out`
        // (42.0).
        let recorded = log.lock().unwrap().clone();
        assert!(
            recorded.contains(&(in_idx, 21.0)),
            "expected In=21.0 change; got {recorded:?}"
        );
        assert!(
            recorded.contains(&(out_idx, 42.0)),
            "expected re-entrant Out=42.0 change; got {recorded:?}"
        );

        drop(widget);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }
    noesis_runtime::shutdown();
}
