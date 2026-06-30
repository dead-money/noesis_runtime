//! `Selector::SelectionChanged` push subscription: programmatically moving a
//! bound `ListBox`'s selection fires the Rust callback, and dropping the RAII
//! token unsubscribes (a later change no longer fires it).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noesis_runtime::binding::ObservableCollection;
use noesis_runtime::events::subscribe_selection_changed;
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="400">
  <ListBox x:Name="LB" Height="300"/>
</Grid>"##;

#[test]
fn selector_selection_changed() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut coll = ObservableCollection::new();
        coll.push_string("A");
        coll.push_string("B");
        coll.push_string("C");

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

        let fired = Arc::new(AtomicUsize::new(0));
        let fired_cb = Arc::clone(&fired);
        let sub = subscribe_selection_changed(&lb, move || {
            fired_cb.fetch_add(1, Ordering::SeqCst);
        })
        .expect("ListBox is a Selector");

        // Move selection to item 0: SelectionChanged must fire.
        let item0 = coll.get(0).expect("item 0");
        // SAFETY: coll outlives lb; item0 is a live element of the bound source.
        assert!(unsafe { lb.set_selected_item(item0.as_ptr()) });
        for i in 10..=12 {
            view.update(f64::from(i) * 0.016);
        }
        let after_first = fired.load(Ordering::SeqCst);
        assert!(
            after_first >= 1,
            "SelectionChanged did not fire on the first selection (count = {after_first})"
        );

        // A different item: the callback fires again.
        let item2 = coll.get(2).expect("item 2");
        // SAFETY: as above.
        assert!(unsafe { lb.set_selected_item(item2.as_ptr()) });
        for i in 13..=15 {
            view.update(f64::from(i) * 0.016);
        }
        assert!(
            fired.load(Ordering::SeqCst) > after_first,
            "SelectionChanged did not fire on the second selection"
        );

        // Drop the subscription: a further selection change must NOT fire it.
        drop(sub);
        let baseline = fired.load(Ordering::SeqCst);
        let item1 = coll.get(1).expect("item 1");
        // SAFETY: as above.
        assert!(unsafe { lb.set_selected_item(item1.as_ptr()) });
        for i in 16..=18 {
            view.update(f64::from(i) * 0.016);
        }
        assert_eq!(
            fired.load(Ordering::SeqCst),
            baseline,
            "callback fired after the subscription was dropped"
        );

        content.clear_items_source();
        drop(content);
        view.deactivate();
        drop(view);
        drop(coll);
    }

    noesis_runtime::shutdown();
}
