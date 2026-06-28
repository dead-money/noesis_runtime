// TODO §8 — Selector selection + ItemsControl direct items mutation.
//
// One headless `#[test]` (the "init once per process" rule). It drives real
// controls inside a live `View` and asserts behaviour that a no-op stub would
// fail:
//
//   * A `ListBox` bound to a Rust `ObservableSource` of three boxed strings:
//     `set_selected_index(2)` round-trips, and `selected_item()` is *pointer-
//     identical* to the third source element (read back through Noesis).
//     Clearing (`-1`) and an out-of-range index (coerced to `-1`) are checked.
//   * An `ItemsControl`'s own `Items`: add two strings (count==2, both realized
//     after layout), remove one (count==1), clear (count==0).
//   * Negatives: a non-`Selector` reports `selected_index()==None`; a
//     non-`ItemsControl` reports `items_count()==None`.

use dm_noesis_runtime::binding::ObservableCollection;
use dm_noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="400">
  <StackPanel>
    <ListBox x:Name="LB" Height="180"/>
    <ItemsControl x:Name="IC" Height="180"/>
  </StackPanel>
</Grid>"##;

#[test]
fn selector_and_items_mutation() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // The source must outlive the ItemsControl that reads it.
        let mut coll = ObservableCollection::new();
        assert_eq!(coll.push_string("A"), Some(0));
        assert_eq!(coll.push_string("B"), Some(1));
        assert_eq!(coll.push_string("C"), Some(2));

        let root = FrameworkElement::parse(XAML).expect("parse XAML");
        let mut view = View::create(root);
        view.set_size(200, 400);
        view.activate();
        // Initial layout pass before touching the controls.
        for i in 1..=4 {
            view.update(f64::from(i) * 0.016);
        }

        let content = view.content().expect("view content");
        let mut lb = content.find_name("LB").expect("find ListBox");
        let mut ic = content.find_name("IC").expect("find ItemsControl");

        // -- Selector via ItemsSource + selected-item identity --
        // SAFETY: coll outlives lb (dropped at end of scope after the view).
        assert!(unsafe { lb.set_items_source(coll.raw()) });
        for i in 5..=9 {
            view.update(f64::from(i) * 0.016);
        }
        assert_eq!(lb.items_count(), Some(3), "ListBox sees 3 source items");

        assert!(lb.set_selected_index(2));
        assert_eq!(lb.selected_index(), Some(2));
        let sel = lb.selected_item().expect("selected item non-null");
        let item2 = coll.get(2).expect("source item 2");
        assert_eq!(
            sel, item2,
            "selected_item must be the third source element (identity through Noesis)"
        );

        // Clearing the selection.
        assert!(lb.set_selected_index(-1));
        assert_eq!(lb.selected_index(), Some(-1));
        assert!(lb.selected_item().is_none(), "no selected item after clear");

        // Out-of-range index is coerced by Noesis to -1 (empty selection).
        assert!(lb.set_selected_index(99));
        assert_eq!(
            lb.selected_index(),
            Some(-1),
            "out-of-range index coerces to -1"
        );

        // -- ItemsControl direct Items mutation --
        assert_eq!(ic.items_add_string("X"), Some(0));
        assert_eq!(ic.items_add_string("Y"), Some(1));
        assert_eq!(ic.items_count(), Some(2), "two items in the collection");
        // The non-virtualizing ItemsControl realizes all items after a layout
        // pass — a genuine signal that change-notification reached the control.
        for i in 10..=16 {
            view.update(f64::from(i) * 0.016);
        }
        assert_eq!(
            ic.realized_item_count(),
            Some(2),
            "both items realized after layout"
        );

        assert!(ic.items_remove_at(0));
        assert_eq!(ic.items_count(), Some(1), "one item after remove");
        assert!(!ic.items_remove_at(5), "out-of-range remove is rejected");

        assert!(ic.items_clear());
        assert_eq!(ic.items_count(), Some(0), "empty after clear");

        // -- Negatives: wrong control type degrades to None --
        assert_eq!(ic.selected_index(), None, "ItemsControl is not a Selector");
        assert_eq!(
            content.items_count(),
            None,
            "the Grid root is not an ItemsControl"
        );

        drop(lb);
        drop(ic);
        drop(content);
        drop(view);
        // Last source reference drops here.
        drop(coll);
    }

    dm_noesis_runtime::shutdown();
}
