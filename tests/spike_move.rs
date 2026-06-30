//! Spike-Move: does a `ListBox`'s selection ride a *reordered object row* to
//! its new slot?
//!
//! This is the low-level gate for the ECS-UI list contract ("selection survives
//! a Move; Reset is the enemy"). Unlike `observable_collection_move` (string
//! items), the rows here are real `ClassInstance` objects with a bound `Name`
//! DP (exactly the shape a per-`Entity` row object takes), so we can capture
//! the *selected item's `ClassInstance` pointer* and prove currency follows the
//! object identity, not the slot index.
//!
//! 5 rows, select index 2, capture the selected pointer, `Move(2 -> 0)`, pump.
//! If the selection still points at the SAME object and now reports index 0,
//! Noesis raised a real `NotifyCollectionChangedAction.Move` and relocated the
//! existing container. A Remove+Add reconcile would have cleared selection
//! (index -1) instead.

use noesis_runtime::binding::ObservableCollection;
use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="400">
  <ListBox x:Name="LB" Height="300">
    <ListBox.ItemTemplate>
      <DataTemplate>
        <TextBlock Text="{Binding Name}" Height="20"/>
      </DataTemplate>
    </ListBox.ItemTemplate>
  </ListBox>
</Grid>"##;

// Rows drive DPs directly; no callback observation needed.
struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn spike_move_selection_survives_object_reorder() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = ClassBuilder::new("DmSpike.MoveRow", ClassBase::Freezable, Noop);
        let name_prop = builder.add_property("Name", PropType::String);
        let reg = builder.register().expect("register MoveRow");

        // Five live row objects, held for the whole scene lifetime.
        let mut rows = Vec::new();
        let mut coll = ObservableCollection::new();
        for i in 0..5 {
            let inst = reg.create_instance().expect("create_instance");
            inst.handle().set_string(name_prop, &format!("R{i}"));
            assert_eq!(coll.push_object(&inst), Some(i));
            rows.push(inst);
        }
        assert_eq!(coll.len(), 5);

        let root = FrameworkElement::parse(XAML).expect("parse XAML");
        let mut view = View::create(root);
        view.set_size(200, 400);
        view.activate();
        for i in 1..=4 {
            view.update(f64::from(i) * 0.016);
        }

        let mut content = view.content().expect("view content");
        let mut lb = content.find_name("LB").expect("find ListBox");
        assert!(lb.set_items_source(&coll));
        for i in 5..=9 {
            view.update(f64::from(i) * 0.016);
        }
        assert_eq!(lb.items_count(), Some(5));

        let r2_ptr = coll.get(2).expect("row 2");

        assert!(lb.set_selected_index(2));
        for i in 10..=12 {
            view.update(f64::from(i) * 0.016);
        }
        let captured = lb.selected_item().expect("a selected item at index 2");
        assert_eq!(
            captured, r2_ptr,
            "selected_item at index 2 must be the R2 object"
        );
        assert_eq!(lb.selected_index(), Some(2));

        // Move R2 from slot 2 to slot 0: [R0,R1,R2,R3,R4] -> [R2,R0,R1,R3,R4].
        assert!(coll.move_item(2, 0), "move_item(2, 0)");
        for i in 13..=18 {
            view.update(f64::from(i) * 0.016);
        }

        // The collection order really changed (R2 is now slot 0).
        assert_eq!(coll.get(0), Some(r2_ptr), "R2 slid to the front");

        // The DECISIVE checks: currency rode the moved object.
        let still_same_object = lb.selected_item() == Some(captured);
        let index_followed = lb.selected_index() == Some(0);
        let move_selection_survives = still_same_object && index_followed;

        eprintln!(
            "SPIKE-MOVE: selected_item==captured? {still_same_object}; \
             selected_index==0? {index_followed} (got {:?}); \
             moveSelectionSurvives={move_selection_survives}",
            lb.selected_index()
        );

        assert!(
            move_selection_survives,
            "selection did not ride the moved object: same_object={still_same_object}, \
             index_followed={index_followed}"
        );

        // Teardown: release the ItemsSource ref before the collection / rows.
        content.clear_items_source();
        drop(lb);
        drop(content);
        view.deactivate();
        drop(view);
        coll.clear();
        drop(coll);
        drop(rows);
        drop(reg);
    }

    noesis_runtime::shutdown();
}
