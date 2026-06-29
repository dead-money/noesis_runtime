//! `ICollectionView` current-item navigation.
//!
//! A [`CollectionViewSource`] wraps a source list (e.g. an
//! [`ObservableCollection`]) and lazily produces a [`CollectionView`], an
//! `ICollectionView`, over it. The view tracks a *current item*: the
//! record-management surface WPF/Noesis controls (a `Selector`'s
//! `IsSynchronizedWithCurrentItem`, master/detail bindings) resolve against.
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
//! Sorting, filtering and grouping remain a genuine SDK limitation in 3.2.13
//! (no programmatic `SortDescription` collection or `Filter` delegate is
//! exposed), so only current-item navigation + `Refresh` are surfaced here.

use core::ptr::NonNull;
use std::ffi::{CStr, c_void};

use crate::binding::ObservableCollection;
use crate::ffi::{
    ClickFn, noesis_base_component_release, noesis_collection_view_count,
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

/// A code-built `Noesis::CollectionViewSource`: the proxy that produces a
/// [`CollectionView`] over a source list. Owns a `+1` reference released on
/// drop.
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
    /// Create an empty `CollectionViewSource` (no source set yet).
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails (returns null).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { noesis_collection_view_source_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_collection_view_source_create returned null"),
        }
    }

    /// Raw `Noesis::CollectionViewSource*` (a `BaseComponent*`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Point this source at an [`ObservableCollection`]; the view is (re)built
    /// over it. Noesis stores its own reference to the collection, so the
    /// collection handle must stay alive for the bindings to keep resolving but
    /// is otherwise independent.
    pub fn set_source(&mut self, source: &ObservableCollection) -> bool {
        // SAFETY: both pointers are live for the call; Noesis takes its own ref.
        unsafe { noesis_collection_view_source_set_source(self.ptr.as_ptr(), source.raw()) }
    }

    /// Point this source at an arbitrary list `BaseComponent*`. Pass
    /// `core::ptr::null_mut()` to clear the source.
    ///
    /// # Safety
    ///
    /// `source` must be null or a live `Noesis::BaseComponent*` implementing
    /// `IList` that outlives the call; Noesis takes its own reference.
    pub unsafe fn set_source_raw(&mut self, source: *mut c_void) -> bool {
        // SAFETY: self.ptr live; source per # Safety.
        unsafe { noesis_collection_view_source_set_source(self.ptr.as_ptr(), source) }
    }

    /// The [`CollectionView`] currently associated with this source
    /// (`CollectionViewSource::GetView`), `AddRef`'d so Rust owns it. `None` if
    /// no source has been set yet. Set a source with [`set_source`](Self::set_source)
    /// first.
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

/// A `Noesis::CollectionView` (an `ICollectionView`) over a source list. Owns a
/// `+1` reference released on drop. Obtained from
/// [`CollectionViewSource::view`].
///
/// The navigation methods mirror `ICollectionView`. Each `move_current_to_*`
/// returns the raw `bool` Noesis reports for the move; its exact meaning at the
/// boundaries is an SDK detail, so query the resulting state with
/// [`current_position`](Self::current_position),
/// [`current_item`](Self::current_item),
/// [`is_current_before_first`](Self::is_current_before_first) and
/// [`is_current_after_last`](Self::is_current_after_last), which re-read the
/// live view after each move.
pub struct CollectionView {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CollectionView {}

impl CollectionView {
    /// Raw `Noesis::CollectionView*` (a `BaseComponent*`). Borrowed for the
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

    /// Ordinal position of the current item. By the `ICollectionView` contract
    /// this is `-1` when the cursor is *before the first* record and `count()`
    /// when it is *after the last*.
    #[must_use]
    pub fn current_position(&self) -> i32 {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_current_position(self.ptr.as_ptr()) }
    }

    /// The current item, `AddRef`'d back out of the live view, or `None` if the
    /// cursor is off the ends of the collection.
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

    /// Move the cursor to the first record. See the type docs for the return
    /// value; query [`current_position`](Self::current_position) for the result.
    pub fn move_current_to_first(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_first(self.ptr.as_ptr()) }
    }

    /// Move the cursor to the last record. See the type docs for the return value.
    pub fn move_current_to_last(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_last(self.ptr.as_ptr()) }
    }

    /// Move the cursor to the next record (lands *after the last* when called at
    /// the end; check [`is_current_after_last`](Self::is_current_after_last)).
    pub fn move_current_to_next(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_next(self.ptr.as_ptr()) }
    }

    /// Move the cursor to the previous record (lands *before the first* when
    /// called at the start; check
    /// [`is_current_before_first`](Self::is_current_before_first)).
    pub fn move_current_to_previous(&self) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_previous(self.ptr.as_ptr()) }
    }

    /// Move the cursor to the record at `position` (`-1` = before first,
    /// `count()` = after last). See the type docs for the return value.
    pub fn move_current_to_position(&self, position: i32) -> bool {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_move_current_to_position(self.ptr.as_ptr(), position) }
    }

    /// Recreate the view (`ICollectionView::Refresh`).
    pub fn refresh(&self) {
        // SAFETY: self.ptr is a live CollectionView*.
        unsafe { noesis_collection_view_refresh(self.ptr.as_ptr()) }
    }

    /// Subscribe `handler` to the view's `CurrentChanged` event, fired after the
    /// current item changes (e.g. from any `move_current_to_*`). The returned
    /// [`CurrentChangedSubscription`] keeps the handler installed until dropped.
    ///
    /// # Panics
    ///
    /// Panics if `Box::into_raw` returns null (an internal logic error).
    #[must_use]
    pub fn subscribe_current_changed<H: CurrentChangedHandler>(
        &self,
        handler: H,
    ) -> Option<CurrentChangedSubscription> {
        // Double-Box gives a stable thin pointer for the C ABI userdata, same as
        // events::subscribe_click.
        let outer: Box<Box<dyn CurrentChangedHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(outer);
        // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked.
        let token = unsafe {
            noesis_collection_view_subscribe_current_changed(
                self.ptr.as_ptr(),
                current_changed_trampoline,
                userdata.cast(),
            )
        };
        if let Some(token) = NonNull::new(token) {
            Some(CurrentChangedSubscription {
                token,
                userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
            })
        } else {
            // Subscription failed (not a view). Reclaim the leaked userdata.
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

/// The current item of a [`CollectionView`], `AddRef`'d so Rust owns it.
/// Released on drop. Use [`raw`](Self::raw) for pointer-identity checks against
/// the source item, or one of [`as_string`](Self::as_string) /
/// [`as_bool`](Self::as_bool) / [`as_i32`](Self::as_i32) /
/// [`as_f64`](Self::as_f64) to unbox a boxed primitive item (each returns `None`
/// on a type mismatch).
pub struct CurrentItem {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CurrentItem {}

impl CurrentItem {
    /// Raw `Noesis::BaseComponent*` of the item. Borrowed for the lifetime of
    /// `self`. Compare against an [`ObservableCollection::get`] pointer to
    /// confirm identity.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Unbox the item as a `String` if it is a boxed string (the common item
    /// type from [`ObservableCollection::push_string`]), else `None`.
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

    /// Unbox the item as a `bool` if it is a boxed `bool` (e.g. from
    /// [`ObservableCollection::push_bool`](crate::binding::ObservableCollection::push_bool)),
    /// else `None`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox the item as an `i32` if it is a boxed `int` (e.g. from
    /// [`ObservableCollection::push_i32`](crate::binding::ObservableCollection::push_i32)),
    /// else `None`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.ptr is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox the item as an `f64` if it is a boxed `double` (e.g. from
    /// [`ObservableCollection::push_f64`](crate::binding::ObservableCollection::push_f64)),
    /// else `None`.
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

/// Rust-side `CurrentChanged` handler. Implementors receive a single `()`
/// notification each time the view's current item changes.
///
/// Takes `&self` (re-entrant-safe; use interior mutability for handler state).
pub trait CurrentChangedHandler: Send + 'static {
    /// Called after the current item changed.
    fn on_current_changed(&self);
}

impl<F: Fn() + Send + 'static> CurrentChangedHandler for F {
    fn on_current_changed(&self) {
        self();
    }
}

/// RAII subscription to a [`CollectionView`]'s `CurrentChanged` event. While
/// alive, the registered handler stays installed; drop it to unsubscribe.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct CurrentChangedSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn CurrentChangedHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CurrentChangedSubscription {}

impl Drop for CurrentChangedSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe; unsubscribe
        // detaches the delegate before we reclaim the handler box.
        unsafe {
            noesis_collection_view_unsubscribe_current_changed(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
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

// Keep the `ClickFn` reuse honest: the trampoline must match its shape.
const _: ClickFn = current_changed_trampoline;
