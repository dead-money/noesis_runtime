//! `ObservableCollection::move_item` maps to Noesis's real
//! `BaseObservableCollection::Move` (a `NotifyCollectionChangedAction.Move`, not
//! a Remove+Add pair). Two checks share one Noesis lifecycle:
//!
//!   1. Bare reorder: moving an item permutes the collection (order changes,
//!      object identity is preserved, `Count` is unchanged).
//!   2. Bound `ListBox`: the selection rides the moved item to its new slot,
//!      which a Remove+Add reconcile (raising Reset/Remove) would drop. This is
//!      the "selection survives a reorder" guarantee the ECS-UI list contract
//!      leans on.

use noesis_runtime::binding::{ObservableCollection, box_string};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="400">
  <ListBox x:Name="LB" Height="300"/>
</Grid>"##;

#[test]
fn observable_collection_move() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // Keep owning handles so we can compare object identity after the move.
        let a = box_string("A");
        let b = box_string("B");
        let c = box_string("C");

        let mut coll = ObservableCollection::new();
        // SAFETY: a/b/c outlive this scope; each raw() is a live boxed value.
        unsafe {
            assert_eq!(coll.push_component(a.raw()), Some(0));
            assert_eq!(coll.push_component(b.raw()), Some(1));
            assert_eq!(coll.push_component(c.raw()), Some(2));
        }
        assert_eq!(coll.len(), 3);

        let id_a = coll.get(0).expect("item 0");
        let id_b = coll.get(1).expect("item 1");
        let id_c = coll.get(2).expect("item 2");

        // Move A (front) to the back: [A,B,C] -> [B,C,A].
        assert!(
            coll.move_item(0, 2),
            "move_item(0, 2) on a 3-item collection"
        );
        assert_eq!(coll.len(), 3, "Move must not change Count");
        assert_eq!(coll.get(0), Some(id_b), "B slid to the front");
        assert_eq!(coll.get(1), Some(id_c), "C slid up");
        assert_eq!(coll.get(2), Some(id_a), "A moved to the back (same object)");

        // Out-of-range indices are rejected, leaving the order intact.
        assert!(!coll.move_item(0, 3), "new_index == len is out of range");
        assert!(!coll.move_item(9, 0), "old_index out of range");
        assert_eq!(coll.get(2), Some(id_a));

        coll.clear();
        drop((a, b, c));
    }

    {
        let a = box_string("Alpha");
        let b = box_string("Beta");
        let c = box_string("Gamma");

        let mut coll = ObservableCollection::new();
        // SAFETY: a/b/c outlive the ListBox bound below.
        unsafe {
            coll.push_component(a.raw());
            coll.push_component(b.raw());
            coll.push_component(c.raw());
        }

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
        assert_eq!(lb.items_count(), Some(3));

        let beta = coll.get(1).expect("Beta");
        // SAFETY: coll outlives lb; beta is a live element of the bound source.
        assert!(unsafe { lb.set_selected_item(beta.as_ptr()) });
        assert_eq!(lb.selected_index(), Some(1));

        // Move Beta to the front: [Alpha,Beta,Gamma] -> [Beta,Alpha,Gamma].
        assert!(coll.move_item(1, 0));
        for i in 10..=14 {
            view.update(f64::from(i) * 0.016);
        }

        // A real Move relocates the existing container, so the selection rides
        // Beta to index 0 (still the same object). A Remove+Add would have
        // cleared it (index -1) or pinned it to the wrong row.
        assert_eq!(
            lb.selected_item(),
            Some(beta),
            "selection tracks the moved object (identity preserved)"
        );
        assert_eq!(
            lb.selected_index(),
            Some(0),
            "selection followed Beta to its new slot"
        );

        content.clear_items_source();
        drop(content);
        view.deactivate();
        drop(view);
        drop(coll);
        drop((a, b, c));
    }

    noesis_runtime::shutdown();
}
