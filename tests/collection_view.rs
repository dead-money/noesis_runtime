//! Phase 6 — `ICollectionView` current-item navigation over a populated source.
//!
//! Builds an `ObservableCollection` of boxed strings, wraps it in a
//! `CollectionViewSource`, and drives the produced `CollectionView`'s current
//! item through every navigation method. Each assertion re-reads the LIVE view
//! (`current_position`, `current_item`, the off-the-ends flags) so a stubbed
//! shim fails the round-trip; pointer identity between the current item and the
//! source slot, plus a `CurrentChanged` counter, prove the FFI crossing.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p noesis_runtime --test collection_view -- --nocapture`

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::binding::ObservableCollection;
use noesis_runtime::collection_view::CollectionViewSource;

#[test]
fn collection_view_current_item_navigation() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut list = ObservableCollection::new();
        list.push_string("alpha");
        list.push_string("beta");
        list.push_string("gamma");

        let mut cvs = CollectionViewSource::new();
        assert!(
            cvs.set_source(&list),
            "set_source on a CollectionViewSource"
        );

        let view = cvs
            .view()
            .expect("GetView returns a CollectionView once Source is set");
        assert_eq!(view.count(), 3, "view sees all 3 source records");

        // CurrentChanged counter — proves the event delegate crossed the FFI.
        let counter = Arc::new(AtomicU32::new(0));
        let counter_cb = Arc::clone(&counter);
        let _sub = view
            .subscribe_current_changed(move || {
                counter_cb.fetch_add(1, Ordering::SeqCst);
            })
            .expect("subscribe to CurrentChanged");

        // First.
        assert!(view.move_current_to_first(), "first is a valid record");
        assert_eq!(view.current_position(), 0);
        assert_eq!(
            view.current_item().and_then(|i| i.as_string()).as_deref(),
            Some("alpha"),
            "current item is alpha"
        );

        // Next.
        assert!(view.move_current_to_next());
        assert_eq!(view.current_position(), 1);
        assert_eq!(
            view.current_item().and_then(|i| i.as_string()).as_deref(),
            Some("beta"),
        );

        // Last.
        assert!(view.move_current_to_last());
        assert_eq!(view.current_position(), 2);
        assert_eq!(
            view.current_item().and_then(|i| i.as_string()).as_deref(),
            Some("gamma"),
        );

        // Past the end — the cursor lands in the well-defined "after last"
        // state (position == count, no current item).
        let _ = view.move_current_to_next();
        assert!(view.is_current_after_last(), "cursor is after the last");
        assert!(
            view.current_item().is_none(),
            "no current item past the end"
        );
        assert_eq!(view.current_position(), 3, "after-last position is count");

        // Absolute position.
        assert!(view.move_current_to_position(1));
        assert_eq!(view.current_position(), 1);
        assert_eq!(
            view.current_item().and_then(|i| i.as_string()).as_deref(),
            Some("beta"),
        );

        // Previous, then before the beginning.
        let _ = view.move_current_to_previous();
        assert_eq!(view.current_position(), 0);
        let _ = view.move_current_to_previous();
        assert!(view.is_current_before_first(), "cursor is before the first");
        assert!(
            view.current_item().is_none(),
            "no current item before the start"
        );
        assert_eq!(view.current_position(), -1, "before-first position is -1");

        // Pointer identity: the current item is the very object stored in the
        // source collection, not a copy.
        assert!(view.move_current_to_first());
        let item = view.current_item().expect("current item");
        let src0 = list.get(0).expect("source[0]");
        assert_eq!(
            item.raw(),
            src0.as_ptr(),
            "current item is the same boxed object as source[0]"
        );

        // Refresh re-creates the view without crashing.
        view.refresh();

        assert!(
            counter.load(Ordering::SeqCst) > 0,
            "CurrentChanged fired at least once during navigation"
        );
    }

    noesis_runtime::shutdown();
}
