//! View models that expose Rust data to XAML bindings.
//!
//! A plain view model is a binding source, not a UI element. You declare a type
//! with named, typed properties, create instances, and push values into them
//! from Rust. Set an instance as an element's `DataContext` and
//! `{Binding Title}` in XAML reads its `Title` property. Call
//! [`PlainInstance::notify`] after a change and bound targets update.
//!
//! Use [`crate::classes`] instead when you need a custom control that XAML can
//! instantiate and place in the visual tree.
//!
//! ```no_run
//! use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
//! use noesis_runtime::view::FrameworkElement;
//!
//! fn show_score(root: &mut FrameworkElement) {
//!     let mut builder = PlainVmBuilder::new("MyGame.HudVm");
//!     let score = builder.add_property("Score", PlainType::Int32);
//!     let class = builder.register().expect("type name not yet registered");
//!
//!     let vm = class.create_instance().expect("instance");
//!     vm.set(score, PlainValue::Int32(0));
//!     assert!(vm.set_data_context(root));
//!
//!     // Later, after the score changes:
//!     let _ = vm.set_and_notify(score, "Score", PlainValue::Int32(10));
//! }
//! ```
//!
//! Instances keep their type's registration alive, so handles can be dropped
//! in any order.
//!
//! # Threading
//!
//! Bindings read properties and receive change notifications on the thread
//! that drives the view. A [`PlainSetHandler`] runs on that thread too.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};

use crate::ffi::{
    PlainSetFn, noesis_base_component_release, noesis_box_bool, noesis_box_double,
    noesis_box_int32, noesis_box_string, noesis_box_u64, noesis_plain_vm_create_instance,
    noesis_plain_vm_get_value, noesis_plain_vm_notify, noesis_plain_vm_register,
    noesis_plain_vm_register_property, noesis_plain_vm_set_value, noesis_plain_vm_unregister,
    noesis_unbox_bool, noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
    noesis_unbox_u64,
};
use crate::view::FrameworkElement;

/// The type of a view-model property, as bindings see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PlainType {
    Int32 = 0,
    Double = 1,
    Bool = 2,
    String = 3,
    /// Any Noesis object, such as a nested view model.
    BaseComponent = 4,
    /// An unsigned 64-bit integer, useful for IDs such as a Bevy `Entity`'s
    /// bits.
    U64 = 5,
}

/// A value to store in a view-model property with [`PlainInstance::set`].
#[derive(Debug, Clone)]
pub enum PlainValue {
    Int32(i32),
    Double(f64),
    Bool(bool),
    /// A string. Storing one with an interior NUL byte panics.
    String(String),
    U64(u64),
    /// Clears the property.
    Null,
}

impl PlainValue {
    /// A `+1`-owned boxed `BaseComponent*`, or null for [`PlainValue::Null`].
    fn into_boxed(self) -> *mut c_void {
        match self {
            // SAFETY: each box fn returns a +1-owned BaseComponent*.
            PlainValue::Int32(i) => unsafe { noesis_box_int32(i) },
            PlainValue::Double(d) => unsafe { noesis_box_double(d) },
            PlainValue::Bool(b) => unsafe { noesis_box_bool(b) },
            // SAFETY: returns a +1-owned BoxedValue<uint64_t>.
            PlainValue::U64(v) => unsafe { noesis_box_u64(v) },
            PlainValue::String(s) => {
                let cs = CString::new(s).expect("plain value string contained NUL");
                // SAFETY: cs is valid for the call; the C side copies the bytes.
                unsafe { noesis_box_string(cs.as_ptr()) }
            }
            PlainValue::Null => ptr::null_mut(),
        }
    }
}

/// A value the UI wrote to a view-model property, passed to a
/// [`PlainSetHandler`]. The typed accessors return `None` if the value is null
/// or of another type.
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

    /// The value as an `i32`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let p = self.0?;
        let mut out = 0i32;
        // SAFETY: p is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_int32(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as an `f64`.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let p = self.0?;
        let mut out = 0.0f64;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_double(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as a `u64`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        let p = self.0?;
        let mut out = 0u64;
        // SAFETY: p is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_u64(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as a `bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let p = self.0?;
        let mut out = false;
        // SAFETY: as above.
        let ok = unsafe { noesis_unbox_bool(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as a string, borrowed for the callback. `None` also for
    /// invalid UTF-8.
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

/// Observes values the UI writes back through `TwoWay` or `OneWayToSource`
/// bindings. The value is already stored when the handler runs; `get_*` reads
/// return it. Closures of the same signature implement it.
pub trait PlainSetHandler: Send + 'static {
    /// `prop_index` is the index [`PlainVmBuilder::add_property`] returned.
    fn on_set(&self, prop_index: u32, value: &PlainValueRef);
}

impl<F> PlainSetHandler for F
where
    F: Fn(u32, &PlainValueRef) + Send + 'static,
{
    fn on_set(&self, prop_index: u32, value: &PlainValueRef) {
        self(prop_index, value);
    }
}

/// SAFETY: `userdata` is the `Box<Box<dyn PlainSetHandler>>` leaked in
/// `PlainVmBuilder::register`, alive until the free trampoline runs.
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

/// SAFETY: `userdata` was produced by `PlainVmBuilder::register` and C++ owns
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

/// Declares a view-model type. Add properties, then call
/// [`register`](Self::register).
pub struct PlainVmBuilder {
    name: CString,
    props: Vec<(CString, PlainType)>,
    handler: Option<Box<dyn PlainSetHandler>>,
}

impl PlainVmBuilder {
    /// Starts a type named `name`, which must not match any type already
    /// registered with Noesis.
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

    /// Adds a property named `name`, the name XAML bindings use. Returns its
    /// index for [`PlainInstance::set`] and the `get_*` accessors. Indices
    /// count up from 0 in the order properties are added.
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

    /// Sets a handler for values the UI writes back. Not needed for `OneWay`
    /// bindings.
    #[must_use]
    pub fn on_set<H: PlainSetHandler>(mut self, handler: H) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Registers the type. Returns `None` if the name is already taken or a
    /// property could not be registered. The name stays registered for the
    /// rest of the process, even after the returned class is dropped.
    #[must_use]
    pub fn register(self) -> Option<PlainVmClass> {
        // Double box: a thin pointer for the C ABI. No handler means null
        // userdata and no free callback.
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
            // Registration failed; reclaim the leaked handler box, since C++
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
                // A property failed; unregister to release our +1 (and free the
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

/// A registered view-model type; creates instances. Live instances keep the
/// registration alive after this handle drops.
pub struct PlainVmClass {
    token: NonNull<c_void>,
    prop_count: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PlainVmClass {}

impl PlainVmClass {
    /// Creates an instance with every property unset. Returns `None` only if
    /// Noesis fails to create it.
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
        // last reference (possibly a live instance) drops.
        unsafe { noesis_plain_vm_unregister(self.token.as_ptr()) }
    }
}

/// An instance of a view-model type, usable as a binding source. Set it as a
/// `DataContext` with [`Self::set_data_context`], or pass its
/// [`raw`](Self::raw) pointer to any API taking a `BaseComponent*`. The handle
/// holds one reference, released on drop.
pub struct PlainInstance {
    ptr: NonNull<c_void>,
    prop_count: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PlainInstance {}

impl PlainInstance {
    /// The underlying `Noesis::BaseComponent*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Stores `value` in property `prop_index` without notifying bindings;
    /// follow with [`Self::notify`], or use [`Self::set_and_notify`]. Returns
    /// `false` if `prop_index` is out of range. The value is not checked
    /// against the property's [`PlainType`].
    ///
    /// # Panics
    ///
    /// Panics if `value` is a [`PlainValue::String`] containing an interior NUL
    /// byte.
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

    /// Tells bindings that property `prop_name` changed, so they read it again.
    /// The name is not checked against the type's properties. Always returns
    /// `true`.
    ///
    /// # Panics
    ///
    /// Panics if `prop_name` contains an interior NUL byte.
    pub fn notify(&self, prop_name: &str) -> bool {
        let c = CString::new(prop_name).expect("property name contained NUL");
        // SAFETY: ptr is live; c is valid for the call.
        unsafe { noesis_plain_vm_notify(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// [`set`](Self::set), then [`notify`](Self::notify) if the set succeeded.
    /// `prop_name` must be the name of property `prop_index`.
    ///
    /// # Panics
    ///
    /// Panics if `prop_name`, or a string `value`, contains an interior NUL
    /// byte.
    #[must_use = "a false return means the property was not set (prop_index out of range)"]
    pub fn set_and_notify(&self, prop_index: u32, prop_name: &str, value: PlainValue) -> bool {
        self.set(prop_index, value) && self.notify(prop_name)
    }

    /// The current value of property `prop_index` as a `String`. `None` if the
    /// property is unset, out of range, or not a string. Includes values the UI
    /// wrote back.
    #[must_use]
    pub fn get_string(&self, prop_index: u32) -> Option<String> {
        self.get(prop_index)
            .and_then(|v| v.as_str().map(str::to_owned))
    }

    /// The current value of property `prop_index` as an `i32`. `None` if the
    /// property is unset, out of range, or another type.
    #[must_use]
    pub fn get_i32(&self, prop_index: u32) -> Option<i32> {
        self.get(prop_index).and_then(|v| v.as_i32())
    }

    /// The current value of property `prop_index` as an `f64`. `None` if the
    /// property is unset, out of range, or another type.
    #[must_use]
    pub fn get_f64(&self, prop_index: u32) -> Option<f64> {
        self.get(prop_index).and_then(|v| v.as_f64())
    }

    /// The current value of property `prop_index` as a `u64`. `None` if the
    /// property is unset, out of range, or another type.
    #[must_use]
    pub fn get_u64(&self, prop_index: u32) -> Option<u64> {
        self.get(prop_index).and_then(|v| v.as_u64())
    }

    /// The current value of property `prop_index` as a `bool`. `None` if the
    /// property is unset, out of range, or another type.
    #[must_use]
    pub fn get_bool(&self, prop_index: u32) -> Option<bool> {
        self.get(prop_index).and_then(|v| v.as_bool())
    }

    /// `None` if unset or out of range.
    fn get(&self, prop_index: u32) -> Option<OwnedBoxed> {
        // SAFETY: ptr is a live instance; result is +1-owned (or null).
        let raw = unsafe { noesis_plain_vm_get_value(self.ptr.as_ptr(), prop_index) };
        NonNull::new(raw).map(OwnedBoxed)
    }

    /// Sets this instance as `element`'s `DataContext`. Noesis holds its own
    /// reference. Returns `false` if the context was not set.
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

/// A `+1`-owned boxed value, released on drop.
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
    fn as_u64(&self) -> Option<u64> {
        let mut out = 0u64;
        // SAFETY: self.0 is a live boxed BaseComponent*; out is a valid slot.
        let ok = unsafe { noesis_unbox_u64(self.0.as_ptr(), &mut out) };
        ok.then_some(out)
    }
}

impl Drop for OwnedBoxed {
    fn drop(&mut self) {
        // SAFETY: get() returned a +1-owned BaseComponent*; release it.
        unsafe { noesis_base_component_release(self.0.as_ptr()) }
    }
}
