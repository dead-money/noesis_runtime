//! Plain (non-`DependencyObject`) view models for data binding.
//!
//! The bevy-bridge unblocker. A plain view model is a Rust-owned binding
//! source that is **not** a `DependencyObject`: a plain Noesis `BaseComponent`
//! that implements `INotifyPropertyChanged` and carries a synthetic reflection
//! type whose properties resolve — through reflection — to a per-instance value
//! store that Rust pushes into. That is what makes `{Binding Title}` work
//! against a Rust view model used as a `DataContext`, and, paired with a
//! [`PlainInstance::notify`] call, what refreshes a bound UI target when Rust
//! mutates the model.
//!
//! This is the lighter-weight sibling of [`crate::classes`]: where a
//! [`ClassBuilder`](crate::classes::ClassBuilder) synthesizes a
//! `ContentControl` subclass with real `DependencyProperty` metadata (so it can
//! be *instantiated from XAML* and participate in the visual tree), a plain VM
//! is purely a binding *source*. Use a plain VM when all you need is to expose
//! Rust state to `{Binding}` (the common Bevy case: feed game state into the UI)
//! without the weight — or the `DependencyObject` thread affinity — of a full
//! control.
//!
//! # Lifecycle
//!
//! 1. [`PlainVmBuilder::new`] → [`add_property`](PlainVmBuilder::add_property) →
//!    [`register`](PlainVmBuilder::register) → [`PlainVmClass`].
//! 2. [`PlainVmClass::create_instance`] → [`PlainInstance`].
//! 3. [`PlainInstance::set`] a property value, then
//!    [`PlainInstance::set_data_context`] it onto an element (or use the raw
//!    pointer via [`PlainInstance::raw`]) and author `{Binding PropName}`.
//! 4. Mutate from Rust with [`PlainInstance::set`] + [`PlainInstance::notify`]
//!    (or the combined [`PlainInstance::set_and_notify`]); the bound target
//!    refreshes on the next `View::update`.
//! 5. Drop the [`PlainInstance`]s, then the [`PlainVmClass`].
//!
//! # Threading
//!
//! Reflection reads / `PropertyChanged` notifications happen on the thread that
//! drives the `View` (in practice the main thread). The optional
//! [`PlainSetHandler`] (a `TwoWay` writeback hook) fires from inside the binding
//! pump on that same thread.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};

use crate::ffi::{
    PlainSetFn, noesis_base_component_release, noesis_box_bool, noesis_box_double,
    noesis_box_int32, noesis_box_string, noesis_plain_vm_create_instance,
    noesis_plain_vm_get_value, noesis_plain_vm_notify, noesis_plain_vm_register,
    noesis_plain_vm_register_property, noesis_plain_vm_set_value, noesis_plain_vm_unregister,
    noesis_unbox_bool, noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
};
use crate::view::FrameworkElement;

/// Content type of a reflected plain-VM property. Mirrors `noesis_plain_type`
/// in `cpp/noesis_shim.h`; the ordinal is the FFI tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PlainType {
    Int32 = 0,
    Double = 1,
    Bool = 2,
    /// The most common case — bind a `TextBlock.Text` to a Rust `String`.
    String = 3,
    /// An opaque `BaseComponent*` (e.g. a nested view model or a boxed object).
    BaseComponent = 4,
}

/// A value to push into a plain-VM property. The crate boxes it into the
/// `BaseComponent*` the binding engine reads.
#[derive(Debug, Clone)]
pub enum PlainValue {
    Int32(i32),
    Double(f64),
    Bool(bool),
    String(String),
    /// An explicit null (clears the property).
    Null,
}

impl PlainValue {
    /// Box into a `+1`-owned `BaseComponent*` (caller owns the reference), or
    /// null for [`PlainValue::Null`].
    fn into_boxed(self) -> *mut c_void {
        match self {
            // SAFETY: each box fn returns a +1-owned BaseComponent*.
            PlainValue::Int32(i) => unsafe { noesis_box_int32(i) },
            PlainValue::Double(d) => unsafe { noesis_box_double(d) },
            PlainValue::Bool(b) => unsafe { noesis_box_bool(b) },
            PlainValue::String(s) => {
                let cs = CString::new(s).unwrap_or_default();
                // SAFETY: cs is valid for the call; the C side copies the bytes.
                unsafe { noesis_box_string(cs.as_ptr()) }
            }
            PlainValue::Null => ptr::null_mut(),
        }
    }
}

/// A borrowed, boxed value handed to a [`PlainSetHandler`] (a `TwoWay`
/// writeback). The typed accessors return `None` when the boxed runtime type
/// doesn't match / the value is null.
pub struct PlainValueRef(Option<NonNull<c_void>>);

impl PlainValueRef {
    fn new(raw: *mut c_void) -> Self {
        Self(NonNull::new(raw))
    }

    /// Whether the value is null.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// Unbox an `i32`, or `None` on type mismatch / null.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let p = self.0?;
        let mut out = 0i32;
        // SAFETY: p is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_int32(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `f64`, or `None` on type mismatch / null.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let p = self.0?;
        let mut out = 0.0f64;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_double(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox a `bool`, or `None` on type mismatch / null.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let p = self.0?;
        let mut out = false;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_bool(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrowed view of a boxed string, valid for the callback. `None` on type
    /// mismatch / null / non-UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let p = self.0?;
        // SAFETY: p is a live boxed BaseComponent*; the returned pointer
        // borrows Noesis-owned storage valid for the callback.
        let s = unsafe { noesis_unbox_string(p.as_ptr()) };
        if s.is_null() {
            return None;
        }
        // SAFETY: s is a NUL-terminated C string from Noesis.
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}

/// A `TwoWay` / `OneWayToSource` writeback hook: fires when a binding pushes a
/// value from the UI **back** to a plain-VM property. The value is already
/// stored in the instance (a subsequent `get_*` read returns it); this
/// callback only lets the model author observe the edit.
pub trait PlainSetHandler: Send + 'static {
    /// `prop_index` is the dense index from
    /// [`PlainVmBuilder::add_property`]; `value` is the boxed value the UI
    /// pushed (borrowed for the call).
    fn on_set(&self, prop_index: u32, value: &PlainValueRef);
}

/// A bare closure is a [`PlainSetHandler`].
impl<F> PlainSetHandler for F
where
    F: Fn(u32, &PlainValueRef) + Send + 'static,
{
    fn on_set(&self, prop_index: u32, value: &PlainValueRef) {
        self(prop_index, value);
    }
}

/// SAFETY: `userdata` is the `Box<Box<dyn PlainSetHandler>>` leaked in
/// [`PlainVmBuilder::register`], alive until the free trampoline runs.
unsafe extern "C" fn plain_set_trampoline(
    userdata: *mut c_void,
    _instance: *mut c_void,
    prop_index: u32,
    boxed_value: *mut c_void,
) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        let handler = &*userdata.cast::<Box<dyn PlainSetHandler>>();
        let value = PlainValueRef::new(boxed_value);
        handler.on_set(prop_index, &value);
    })
}

/// SAFETY: `userdata` was produced by [`PlainVmBuilder::register`] and C++ owns
/// it; this is the matching `Box::from_raw` that ends that ownership, run
/// exactly once when the registration refcount hits zero.
unsafe extern "C" fn plain_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn PlainSetHandler>>()));
    })
}

/// Builder for a plain-VM type registration.
pub struct PlainVmBuilder {
    name: CString,
    props: Vec<(CString, PlainType)>,
    handler: Option<Box<dyn PlainSetHandler>>,
}

impl PlainVmBuilder {
    /// Begin a registration for a type named `name` (must be unique across all
    /// Noesis-reflected types).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: CString::new(name).expect("plain-VM type name contained NUL"),
            props: Vec::new(),
            handler: None,
        }
    }

    /// Append a reflected property. Returns the dense index used by
    /// [`PlainInstance::set`], the `get_*` accessors, and the
    /// [`PlainSetHandler`]; indices grow from 0 in addition order.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn add_property(&mut self, name: &str, kind: PlainType) -> u32 {
        let idx = self.props.len() as u32;
        self.props.push((
            CString::new(name).expect("property name contained NUL"),
            kind,
        ));
        idx
    }

    /// Install a `TwoWay` writeback hook (see [`PlainSetHandler`]). Optional —
    /// omit it for read-only / `OneWay` view models.
    #[must_use]
    pub fn on_set<H: PlainSetHandler>(mut self, handler: H) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Finalize the registration. Returns `None` if the type name is already
    /// registered or a property registration failed.
    #[must_use]
    pub fn register(self) -> Option<PlainVmClass> {
        // Double-Box for a stable thin pointer across the C ABI, matching the
        // Command / Converter pattern. A `None` handler still donates an empty
        // closure box so the free trampoline has a uniform shape.
        let (userdata, on_set): (*mut c_void, Option<PlainSetFn>) = match self.handler {
            Some(h) => {
                let boxed: Box<Box<dyn PlainSetHandler>> = Box::new(h);
                (Box::into_raw(boxed).cast(), Some(plain_set_trampoline))
            }
            None => (ptr::null_mut(), None),
        };

        // SAFETY: name is a valid C string; userdata ownership transfers to C++
        // (freed via plain_free_trampoline). free_handler is null when there is
        // no userdata.
        let free = if userdata.is_null() {
            None
        } else {
            Some(plain_free_trampoline as crate::ffi::PlainFreeFn)
        };
        let token = unsafe { noesis_plain_vm_register(self.name.as_ptr(), on_set, userdata, free) };

        let Some(token) = NonNull::new(token) else {
            // Registration failed — reclaim the leaked handler box, since C++
            // took no ownership.
            if !userdata.is_null() {
                // SAFETY: userdata came from Box::into_raw above and C++ never
                // stored it (null return).
                unsafe { drop(Box::from_raw(userdata.cast::<Box<dyn PlainSetHandler>>())) };
            }
            return None;
        };

        let mut count = 0u32;
        for (pname, kind) in &self.props {
            // SAFETY: token is live; pname is a valid C string.
            let idx = unsafe {
                noesis_plain_vm_register_property(token.as_ptr(), pname.as_ptr(), *kind as u32)
            };
            if idx == u32::MAX {
                // A property failed — unregister to release our +1 (and free the
                // handler box) rather than leak a half-built type.
                // SAFETY: token is live and owned by us here.
                unsafe { noesis_plain_vm_unregister(token.as_ptr()) };
                return None;
            }
            count += 1;
        }

        Some(PlainVmClass {
            token,
            prop_count: count,
        })
    }
}

/// A registered plain-VM type. Owns the registration's `+1`; dropping it stops
/// new instances being created and releases that reference (live instances keep
/// the registration alive until they drop).
pub struct PlainVmClass {
    token: NonNull<c_void>,
    prop_count: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PlainVmClass {}

impl PlainVmClass {
    /// Create an instance of this type. Returns `None` only on an impossible
    /// null-token invariant.
    #[must_use]
    pub fn create_instance(&self) -> Option<PlainInstance> {
        // SAFETY: token is a live registration handle; the result is a
        // +1-owned BaseComponent*.
        let ptr = unsafe { noesis_plain_vm_create_instance(self.token.as_ptr()) };
        NonNull::new(ptr).map(|ptr| PlainInstance {
            ptr,
            prop_count: self.prop_count,
        })
    }

    /// Number of registered properties.
    #[must_use]
    pub fn property_count(&self) -> u32 {
        self.prop_count
    }
}

impl Drop for PlainVmClass {
    fn drop(&mut self) {
        // SAFETY: token came from noesis_plain_vm_register with +1; this
        // releases exactly that ref. The handler box (if any) is freed once the
        // last reference — possibly a live instance — drops.
        unsafe { noesis_plain_vm_unregister(self.token.as_ptr()) }
    }
}

/// A live plain-VM instance: a binding source. Owns a `+1` reference released on
/// drop. Set it as a `DataContext` ([`Self::set_data_context`]) or hand its
/// [`raw`](Self::raw) pointer to any API taking a `BaseComponent*`.
pub struct PlainInstance {
    ptr: NonNull<c_void>,
    prop_count: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PlainInstance {}

impl PlainInstance {
    /// Raw `Noesis::BaseComponent*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Store `value` as property `prop_index`'s current value. Does **not**
    /// raise the change notification — call [`Self::notify`] (or use
    /// [`Self::set_and_notify`]). Returns `false` if `prop_index` is out of
    /// range.
    pub fn set(&self, prop_index: u32, value: PlainValue) -> bool {
        if prop_index >= self.prop_count {
            return false;
        }
        let boxed = value.into_boxed();
        // SAFETY: ptr is a live instance; the instance takes its OWN ref on
        // `boxed`, so we still own (and must release) our +1 below.
        let ok = unsafe { noesis_plain_vm_set_value(self.ptr.as_ptr(), prop_index, boxed) };
        if !boxed.is_null() {
            // SAFETY: boxed is our +1 from into_boxed; release it (the instance
            // holds its own ref now). Null boxed (PlainValue::Null) is a no-op.
            unsafe { noesis_base_component_release(boxed) };
        }
        ok
    }

    /// Raise `INotifyPropertyChanged.PropertyChanged` for `prop_name`, so every
    /// binding sourced from that property re-reads on the next pump. Returns
    /// `true` once the notification is raised.
    ///
    /// # Panics
    ///
    /// Panics if `prop_name` contains an interior NUL byte.
    pub fn notify(&self, prop_name: &str) -> bool {
        let c = CString::new(prop_name).expect("property name contained NUL");
        // SAFETY: ptr is live; c is valid for the call.
        unsafe { noesis_plain_vm_notify(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Convenience: [`set`](Self::set) then [`notify`](Self::notify).
    ///
    /// # Panics
    ///
    /// Panics if `prop_name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (prop_index out of range)"]
    pub fn set_and_notify(&self, prop_index: u32, prop_name: &str, value: PlainValue) -> bool {
        self.set(prop_index, value) && self.notify(prop_name)
    }

    /// Read the current boxed value of `prop_index` back as a `String` (copying
    /// it). `None` if unset, out of range, or not a boxed string. Reads the
    /// reflection-visible store directly (not through any binding) — handy for
    /// verifying a `TwoWay` writeback landed.
    #[must_use]
    pub fn get_string(&self, prop_index: u32) -> Option<String> {
        self.get(prop_index)
            .and_then(|v| v.as_str().map(str::to_owned))
    }

    /// Read the current boxed value of `prop_index` as an `i32`. `None` if
    /// unset / out of range / type mismatch.
    #[must_use]
    pub fn get_i32(&self, prop_index: u32) -> Option<i32> {
        self.get(prop_index).and_then(|v| v.as_i32())
    }

    /// Read the current boxed value of `prop_index` as an `f64`.
    #[must_use]
    pub fn get_f64(&self, prop_index: u32) -> Option<f64> {
        self.get(prop_index).and_then(|v| v.as_f64())
    }

    /// Read the current boxed value of `prop_index` as a `bool`.
    #[must_use]
    pub fn get_bool(&self, prop_index: u32) -> Option<bool> {
        self.get(prop_index).and_then(|v| v.as_bool())
    }

    /// Fetch a +1-owned boxed value, wrapped so it's released on drop. Returns
    /// `None` if unset / out of range.
    fn get(&self, prop_index: u32) -> Option<OwnedBoxed> {
        // SAFETY: ptr is a live instance; result is +1-owned (or null).
        let raw = unsafe { noesis_plain_vm_get_value(self.ptr.as_ptr(), prop_index) };
        NonNull::new(raw).map(OwnedBoxed)
    }

    /// Set this instance as `element`'s `DataContext`. Noesis takes its own
    /// reference. Returns `false` if `element` is not a `FrameworkElement`.
    #[must_use = "a false return means the data context was not set (element is not a FrameworkElement)"]
    pub fn set_data_context(&self, element: &mut FrameworkElement) -> bool {
        // SAFETY: self.raw() is a live BaseComponent* valid for the call;
        // Noesis stores its own reference.
        unsafe { element.set_data_context_raw(self.raw()) }
    }
}

impl Drop for PlainInstance {
    fn drop(&mut self) {
        // SAFETY: produced by create_instance with +1; releases exactly that.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// RAII wrapper for a `+1`-owned boxed value fetched from the instance store.
struct OwnedBoxed(NonNull<c_void>);

impl OwnedBoxed {
    fn as_str(&self) -> Option<&str> {
        // SAFETY: self.0 is a live +1-owned boxed BaseComponent*; the returned
        // pointer borrows Noesis storage valid while `self` is alive.
        let s = unsafe { noesis_unbox_string(self.0.as_ptr()) };
        if s.is_null() {
            return None;
        }
        // SAFETY: s is a NUL-terminated C string from Noesis.
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
    fn as_i32(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.0 is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_int32(self.0.as_ptr(), &mut out) };
        ok.then_some(out)
    }
    fn as_f64(&self) -> Option<f64> {
        let mut out = 0.0f64;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_double(self.0.as_ptr(), &mut out) };
        ok.then_some(out)
    }
    fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_bool(self.0.as_ptr(), &mut out) };
        ok.then_some(out)
    }
}

impl Drop for OwnedBoxed {
    fn drop(&mut self) {
        // SAFETY: get() returned a +1-owned BaseComponent*; release it.
        unsafe { noesis_base_component_release(self.0.as_ptr()) }
    }
}
