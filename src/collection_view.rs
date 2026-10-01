//! Collection views: a cursor over a list for current-item navigation.
//!
//! A [`CollectionViewSource`] wraps a source list such as an
//! [`ObservableCollection`] and gives you a [`CollectionView`] over it. The
//! view tracks a *current item*, which `Selector` controls with
//! `IsSynchronizedWithCurrentItem` and master/detail bindings follow. Watch it
//! with [`CollectionView::subscribe_current_changed`].
//!
//! ```no_run
//! # use noesis_runtime::binding::ObservableCollection;
//! # use noesis_runtime::collection_view::CollectionViewSource;
//! let mut list = ObservableCollection::new();
//! list.push_string("a");
//! list.push_string("b");
//!
//! let mut cvs = CollectionViewSource::new();
//! cvs.set_source(&list);
//! let view = cvs.view().expect("view");
//! view.move_current_to_first();
//! assert_eq!(view.current_position(), 0);
//! view.move_current_to_next();
//! assert_eq!(view.current_position(), 1);
//! ```
//!
//! Sorting, filtering and grouping are not available: the Noesis 3.2 SDK
//! exposes no programmatic `SortDescription` collection or `Filter` delegate.

use core::ptr::NonNull;
use std::ffi::{CStr, c_void};

use crate::binding::ObservableCollection;
use crate::ffi::{
    ClickFn, SubscriptionFreeFn, noesis_base_component_release, noesis_collection_view_count,
    noesis_collection_view_current_item, noesis_collection_view_current_position,
    noesis_collection_view_is_current_after_last, noesis_collection_view_is_current_before_first,
    noesis_collection_view_move_current_to_first, noesis_collection_view_move_current_to_last,
    noesis_collection_view_move_current_to_next, noesis_collection_view_move_current_to_position,
    noesis_collection_view_move_current_to_previous, noesis_collection_view_refresh,
    noesis_collection_view_source_create, noesis_collection_view_source_get_view,
    noesis_collection_view_source_set_source, noesis_collection_view_subscribe_current_changed,
    noesis_collection_view_unsubscribe_current_changed, noesis_unbox_bool, noesis_unbox_double,
    noesis_unbox_int32, noesis_unbox_string,
};

/// Produces a [`CollectionView`] over a source list. Owns one Noesis
/// reference, released on drop.
pub struct CollectionViewSource {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CollectionViewSource {}

impl Default for CollectionViewSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CollectionViewSource {
    /// Creates a source with no list set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { noesis_collection_view_source_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_collection_view_source_create returned null"),
        }
    }

    /// Raw `Noesis::CollectionViewSource*` (a `BaseComponent*`), borrowed for
    /// the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Sets `source` as the list to view. Noesis takes its own reference, so
    /// the list stays alive even if your handle is dropped; keep the handle to
    /// keep editing it. Returns `true` on success.
    pub fn set_source(&mut self, source: &ObservableCollection) -> bool {
        // SAFETY: both pointers are live for the call; Noesis takes its own ref.
        unsafe { noesis_collection_view_source_set_source(self.ptr.as_ptr(), source.raw()) }
    }

    /// Sets any `IList` object as the source. Null clears it. Returns `true`
    /// on success.
    ///
    /// # Safety
    ///
    /// `source` must be null or a live `Noesis::BaseComponent*` implementing
    /// `IList` that outlives the call; Noesis takes its own reference.
    pub unsafe fn set_source_raw(&mut self, source: *mut c_void) -> bool {
        // SAFETY: self.ptr live; source per # Safety.
        unsafe { noesis_collection_view_source_set_source(self.ptr.as_ptr(), source) }
    }

    /// A view over the current source, or `None` if no list source is set.
    ///
    /// A source hosted in an element tree (for example declared in XAML)
    /// returns its own shared view. A code-built source that is not in a tree
    /// has none, so each call builds a new view with its own cursor: call this
    /// once and keep the result.
    #[must_use]
    pub fn view(&self) -> Option<CollectionView> {
        // SAFETY: self.ptr is a live CollectionViewSource*; result is +1-owned.
        let p = unsafe { noesis_collection_view_source_get_view(self.ptr.as_ptr()) };
        NonNull::new(p).map(|ptr| CollectionView { ptr })
    }
}

impl Drop for CollectionViewSource {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create).
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A cursor over a source list (`Noesis::CollectionView`). Get one from
/// [`CollectionViewSource::view`]. Owns one Noesis reference, released on
/// drop.
///
/// Each `move_current_to_*` returns the `bool` Noesis reports for the move,
/// whose meaning at the ends of the list is not documented by the SDK. Check
/// the result with [`current_position`](Self::current_position),
/// [`current_item`](Self::current_item),
/// [`is_current_before_first`](Self::is_current_before_first) or
/// [`is_current_after_last`](Self::is_current_after_last).
pub struct CollectionView {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CollectionView {}

impl CollectionView {
    /// Raw `Noesis::CollectionView*` (a `BaseComponent*`), borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Number of records in the view.
    #[must_use]
    pub fn count(&self) -> u32 {
        // SAFETY: self.ptr is a live CollectionView*.
        let n = unsafe { noesis_collection_view_count(self.ptr.as_ptr()) };
        u32::try_from(n.max(0)).unwrap_or(0)
    }

    /// Index of the current item: `-1` before the first record, `count()`
    /// after the last.
    #[must_use]
    pub fn current_position(&self) -> i32 {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_current_position(self.ptr.as_ptr()) }
    }

    /// The current item, or `None` if the cursor is off either end of the
    /// list.
    #[must_use]
    pub fn current_item(&self) -> Option<CurrentItem> {
        // SAFETY: self.ptr is a live CollectionView*; result is +1-owned or null.
        let p = unsafe { noesis_collection_view_current_item(self.ptr.as_ptr()) };
        NonNull::new(p).map(|ptr| CurrentItem { ptr })
    }

    /// Whether the cursor is positioned before the first record.
    #[must_use]
    pub fn is_current_before_first(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_is_current_before_first(self.ptr.as_ptr()) }
    }

    /// Whether the cursor is positioned after the last record.
    #[must_use]
    pub fn is_current_after_last(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_is_current_after_last(self.ptr.as_ptr()) }
    }

    /// Moves to the first record. See the type docs for the return value.
    pub fn move_current_to_first(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_first(self.ptr.as_ptr()) }
    }

    /// Moves to the last record. See the type docs for the return value.
    pub fn move_current_to_last(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_last(self.ptr.as_ptr()) }
    }

    /// Moves to the next record. From the last record it moves past the end;
    /// check [`is_current_after_last`](Self::is_current_after_last).
    pub fn move_current_to_next(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_next(self.ptr.as_ptr()) }
    }

    /// Moves to the previous record. From the first record it moves before the
    /// start; check [`is_current_before_first`](Self::is_current_before_first).
    pub fn move_current_to_previous(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_previous(self.ptr.as_ptr()) }
    }

    /// Moves to `position`, where `-1` is before the first record and
    /// `count()` is after the last. See the type docs for the return value.
    pub fn move_current_to_position(&self, position: i32) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_position(self.ptr.as_ptr(), position) }
    }

    /// Rebuilds the view from the source list.
    pub fn refresh(&self) {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_refresh(self.ptr.as_ptr()) }
    }

    /// Calls `handler` after each change of the current item. The handler stays
    /// installed until the returned [`CurrentChangedSubscription`] is dropped,
    /// which is safe to do from inside the handler. Returns `None` if Noesis
    /// refuses the subscription.
    #[must_use]
    pub fn subscribe_current_changed<H: CurrentChangedHandler>(
        &self,
        handler: H,
    ) -> Option<CurrentChangedSubscription> {
        // outer Box: thin pointer for the C ABI userdata
        let outer: Box<Box<dyn CurrentChangedHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(outer);
        // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and
        // donated to the C++ handler (freed via the free trampoline on teardown).
        let token = unsafe {
            noesis_collection_view_subscribe_current_changed(
                self.ptr.as_ptr(),
                current_changed_trampoline,
                userdata.cast(),
                current_changed_free,
            )
        };
        if let Some(token) = NonNull::new(token) {
            Some(CurrentChangedSubscription { token })
        } else {
            // C++ took no ownership on failure
            // SAFETY: userdata came from Box::into_raw moments ago; nothing took it.
            drop(unsafe { Box::from_raw(userdata) });
            None
        }
    }
}

impl Drop for CollectionView {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (get_view).
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// The current item of a [`CollectionView`]. Owns one Noesis reference,
/// released on drop.
///
/// Compare [`raw`](Self::raw) against a source item's pointer for identity, or
/// unbox a primitive item with [`as_string`](Self::as_string),
/// [`as_bool`](Self::as_bool), [`as_i32`](Self::as_i32) or
/// [`as_f64`](Self::as_f64), each of which returns `None` on a type mismatch.
pub struct CurrentItem {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CurrentItem {}

impl CurrentItem {
    /// Raw `Noesis::BaseComponent*` of the item, borrowed for the lifetime of
    /// `self`. Equal to the [`ObservableCollection::get`] pointer of the same
    /// item.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The item as a `String` if it is a boxed string (as added by
    /// [`ObservableCollection::push_string`]), else `None`. Invalid UTF-8 is
    /// replaced with `U+FFFD`.
    #[must_use]
    pub fn as_string(&self) -> Option<String> {
        // SAFETY: self.ptr is a live boxed BaseComponent*.
        let p = unsafe { noesis_unbox_string(self.ptr.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a NUL-terminated string owned by the boxed value.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// The item as a `bool` if it is a boxed `bool` (as added by
    /// [`ObservableCollection::push_bool`]), else `None`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The item as an `i32` if it is a boxed `int32` (as added by
    /// [`ObservableCollection::push_i32`]), else `None`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.ptr is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The item as an `f64` if it is a boxed `double` (as added by
    /// [`ObservableCollection::push_f64`]), else `None`.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let mut out = 0.0f64;
        // SAFETY: self.ptr is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_double(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }
}

impl Drop for CurrentItem {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref by current_item.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Handler for [`CollectionView::subscribe_current_changed`]. Any
/// `Fn() + Send + 'static` closure implements it.
///
/// Takes `&self` because the handler can be re-entered (moving the cursor
/// from inside it fires it again); keep mutable state in a `Cell`, `RefCell`
/// or `Mutex`.
pub trait CurrentChangedHandler: Send + 'static {
    /// Called after the current item changes.
    fn on_current_changed(&self);
}

impl<F: Fn() + Send + 'static> CurrentChangedHandler for F {
    fn on_current_changed(&self) {
        self();
    }
}

/// Keeps a [`CurrentChangedHandler`] installed. Drop it to unsubscribe.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct CurrentChangedSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CurrentChangedSubscription {}

impl Drop for CurrentChangedSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe; unsubscribe detaches the delegate
        // and frees the donated handler box exactly once (deferred if dropping
        // from inside the callback).
        unsafe { noesis_collection_view_unsubscribe_current_changed(self.token.as_ptr()) }
    }
}

/// SAFETY: `userdata` must be the pointer produced by `subscribe_current_changed`
/// and still alive (the [`CurrentChangedSubscription`] hasn't been dropped).
unsafe extern "C" fn current_changed_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // SAFETY: userdata is the Box<Box<dyn CurrentChangedHandler>> leaked in
        // subscribe, alive until the subscription drops. Shared `&`: re-entrant.
        let handler = unsafe { &*userdata.cast::<Box<dyn CurrentChangedHandler>>() };
        handler.on_current_changed();
    });
}

/// Free trampoline for the donated handler box. The C++ handler calls it exactly
/// once when it is destroyed (deferred past any in-flight callback).
///
/// SAFETY: `userdata` is the `Box<Box<dyn CurrentChangedHandler>>` leaked in
/// subscribe; the C++ side invokes this at most once.
unsafe extern "C" fn current_changed_free(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // SAFETY: reclaim the exact box leaked in subscribe; runs once.
        drop(unsafe { Box::from_raw(userdata.cast::<Box<dyn CurrentChangedHandler>>()) });
    });
}

// the C side reuses the click and subscription-free callback types
const _: ClickFn = current_changed_trampoline;
const _: SubscriptionFreeFn = current_changed_free;
