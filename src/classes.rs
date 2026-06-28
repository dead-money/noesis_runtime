//! Register Rust-backed XAML classes (Phase 5.C).
//!
//! This is the Rust analogue of what the Noesis C# / Unity binding does for
//! managed code: it lets you declare new `<myns:Foo>` types whose dependency
//! properties + property-change logic live entirely in Rust. The C++ side
//! synthesizes a `Noesis::TypeClassBuilder` per consumer-named class and
//! installs a Factory creator that returns a per-base trampoline subclass
//! ([`ContentControl`] is the v1 base; sibling bases plug in incrementally).
//!
//! # Lifecycle
//!
//! 1. Call [`init`](crate::init) so Noesis's reflection registry is alive.
//! 2. Build a class with [`ClassBuilder::new`], add DPs with
//!    [`ClassBuilder::add_property`], and finalize with
//!    [`ClassBuilder::register`] → [`ClassRegistration`].
//! 3. Load XAML that references the class by name. Property writes from
//!    XAML / bindings / runtime fire your [`PropertyChangeHandler::on_changed`]
//!    on the main thread.
//! 4. From Rust, mutate the instance via [`Instance::set_int32`] /
//!    [`Instance::set_thickness`] / etc. — useful for "computed" properties
//!    (NineSlicer's `TopLeftViewbox` family is the motivating example).
//! 5. Drop the [`ClassRegistration`] AFTER all live instances are released
//!    (typically at process shutdown). RAII + the `Send`/`Sync` bounds are
//!    deliberately conservative — registrations are cheap and rare.
//!
//! # Threading
//!
//! Property-changed callbacks fire on whatever thread drives the View — in
//! practice the main thread. The handler is stored behind a `Send` trait
//! bound; if you need cross-thread fan-out (e.g. Bevy ECS), keep the handler
//! body small and route to a channel / queue.
//!
//! # Re-entrancy
//!
//! [`Instance::set_*`] calls fire the property-changed callback synchronously
//! if the new value differs. Guard against re-entrancy in the handler if you
//! plan to write back to the same property.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};
use std::sync::Mutex;

use crate::ffi::{
    ClassBase, PropType, dm_noesis_class_register, dm_noesis_class_register_property,
    dm_noesis_class_unregister, dm_noesis_image_source_get_size, dm_noesis_instance_get_property,
    dm_noesis_instance_set_property,
};

/// Free trampoline matching [`crate::ffi::ClassFreeFn`]. The C++ side holds a
/// pointer to this function and invokes it exactly once when the underlying
/// `ClassData` is finally freed (immediately at unregister if no instances
/// exist, or deferred to the last live instance's destruction). Drops the
/// double-boxed `Box<dyn PropertyChangeHandler>` whose ownership was
/// transferred to C++ at registration time, and clears the prop-types
/// scratch slot keyed on the same userdata pointer.
unsafe extern "C" fn class_handler_free_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    forget_prop_types(userdata);
    // SAFETY: `userdata` is `Box::into_raw(Box<Box<dyn PropertyChangeHandler>>)`
    // produced by `ClassBuilder::register`. The C++ ClassData holds the
    // unique ownership; this is the matching `Box::from_raw` that ends it.
    unsafe {
        drop(Box::from_raw(
            userdata as *mut Box<dyn PropertyChangeHandler>,
        ))
    };
}

/// Read width / height of an `ImageSource` value (or any `BaseComponent*`
/// whose runtime type is an `ImageSource` subclass). Returns `None` when
/// the pointer is null or doesn't downcast.
///
/// Useful when a custom-control [`PropertyChangeHandler`] needs the source
/// dimensions to compute derived properties — NineSlicer / ThreeSlicer's
/// `OnSlicesChanged` is the motivating example.
///
/// # Safety
///
/// `image_source` must be a pointer obtained from a [`PropertyValue`]
/// (`ImageSource` or `BaseComponent` variant) or from another live Noesis
/// `BaseComponent`. Caller does not own a ref.
#[must_use]
pub unsafe fn image_source_size(image_source: NonNull<c_void>) -> Option<(f32, f32)> {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    let ok = dm_noesis_image_source_get_size(image_source.as_ptr(), &mut w, &mut h);
    ok.then_some((w, h))
}

/// Per-instance Rust callback. Implementations receive a stable instance
/// pointer (see [`Instance`]) and the index of the changed property — index
/// matches the order in which DPs were added to the class.
pub trait PropertyChangeHandler: Send + 'static {
    fn on_changed(&mut self, instance: Instance, prop_index: u32, value: PropertyValue<'_>);
}

/// Property value as observed by the change callback. Variant matches the
/// [`PropType`] declared at registration time.
///
/// Borrowed variants (`String`, `ImageSource`, `BaseComponent`) reference
/// Noesis-owned storage that may be invalidated by the next layout pass —
/// copy if you need to keep the value past the callback.
#[derive(Debug)]
pub enum PropertyValue<'a> {
    Int32(i32),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(Option<&'a str>),
    Thickness {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    },
    Color {
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    /// Borrowed `Noesis::ImageSource*` (or null). Treat as opaque.
    ImageSource(Option<NonNull<c_void>>),
    /// Borrowed `Noesis::BaseComponent*` (or null). Treat as opaque.
    BaseComponent(Option<NonNull<c_void>>),
}

/// Builder for a single class registration.
pub struct ClassBuilder<H: PropertyChangeHandler> {
    name: CString,
    base: ClassBase,
    handler: H,
    props: Vec<(CString, PropType, OwnedDefault)>,
}

enum OwnedDefault {
    None,
    Int32(i32),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(Option<CString>),
    Thickness([f32; 4]),
    Color([f32; 4]),
    Rect([f32; 4]),
}

impl<H: PropertyChangeHandler> ClassBuilder<H> {
    /// Begin a new class registration. `name` is the XAML-visible type name
    /// (e.g. `"AOR.NineSlicer"`); the XAML namespace mapping
    /// (`xmlns:aor="clr-namespace:AOR"`) lives in the XAML itself.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn new(name: &str, base: ClassBase, handler: H) -> Self {
        Self {
            name: CString::new(name).expect("class name contained NUL"),
            base,
            handler,
            props: Vec::new(),
        }
    }

    /// Append a dependency property. The returned `u32` is the dense index
    /// the change callback receives for this property; indices grow in
    /// addition order starting at 0.
    ///
    /// Defaults are best-effort for v1: scalar / Thickness / Color / Rect
    /// work; `ImageSource` and `BaseComponent` always default to null
    /// (matching AoR's authoring style).
    pub fn add_property(&mut self, name: &str, kind: PropType) -> u32 {
        self.add_property_with(name, kind, PropertyDefault::None)
    }

    /// Same as [`Self::add_property`] but with an explicit default value.
    pub fn add_property_with(
        &mut self,
        name: &str,
        kind: PropType,
        default: PropertyDefault<'_>,
    ) -> u32 {
        let cstr = CString::new(name).expect("property name contained NUL");
        self.props.push((cstr, kind, default.into_owned()));
        self.props.len() as u32 - 1
    }

    /// Finalize the registration. Returns `None` if the C++ side rejected
    /// the registration (most commonly: name already registered, or a
    /// property had a type the v1 FFI doesn't yet support).
    pub fn register(self) -> Option<ClassRegistration> {
        let ClassBuilder {
            name,
            base,
            handler,
            props,
        } = self;
        let prop_types: Vec<PropType> = props.iter().map(|(_, k, _)| *k).collect();

        // Box twice so we have a stable thin pointer for the C ABI userdata,
        // matching the pattern in `events::subscribe_click`.
        let boxed: Box<Box<dyn PropertyChangeHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // Record the prop type list BEFORE the FFI call so the trampoline
        // can decode `value_ptr` if the C++ side fires a callback during
        // registration (e.g. on a default-value initialization).
        record_prop_types(userdata.cast(), prop_types);

        let token = unsafe {
            dm_noesis_class_register(
                name.as_ptr(),
                base,
                prop_changed_trampoline,
                userdata.cast(),
                class_handler_free_trampoline,
            )
        };
        let Some(token) = NonNull::new(token) else {
            // Registration failed before C++ took ownership of the userdata
            // box (the C side returns NULL before storing the pointer on
            // ClassData). Drop locally.
            forget_prop_types(userdata.cast());
            unsafe { drop(Box::from_raw(userdata)) };
            return None;
        };

        for (prop_name, kind, default) in &props {
            let default_ptr = default.as_ffi_ptr();
            let idx = unsafe {
                dm_noesis_class_register_property(
                    token.as_ptr(),
                    prop_name.as_ptr(),
                    *kind,
                    default_ptr,
                )
            };
            if idx == u32::MAX {
                // C++ owns the userdata box now (registered above); calling
                // `dm_noesis_class_unregister` triggers the free trampoline
                // when ClassData's last ref drops, which is right here since
                // no instances were created yet.
                unsafe { dm_noesis_class_unregister(token.as_ptr()) };
                return None;
            }
        }

        Some(ClassRegistration {
            token,
            _name: name,
            num_props: props.len() as u32,
        })
    }
}

/// Default value supplied to [`ClassBuilder::add_property_with`].
#[derive(Debug, Clone, Copy)]
pub enum PropertyDefault<'a> {
    None,
    Int32(i32),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(&'a str),
    Thickness {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    },
    Color {
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

impl PropertyDefault<'_> {
    fn into_owned(self) -> OwnedDefault {
        match self {
            PropertyDefault::None => OwnedDefault::None,
            PropertyDefault::Int32(v) => OwnedDefault::Int32(v),
            PropertyDefault::Float(v) => OwnedDefault::Float(v),
            PropertyDefault::Double(v) => OwnedDefault::Double(v),
            PropertyDefault::Bool(v) => OwnedDefault::Bool(v),
            PropertyDefault::String(s) => {
                let c = CString::new(s).ok();
                OwnedDefault::String(c)
            }
            PropertyDefault::Thickness {
                left,
                top,
                right,
                bottom,
            } => OwnedDefault::Thickness([left, top, right, bottom]),
            PropertyDefault::Color { r, g, b, a } => OwnedDefault::Color([r, g, b, a]),
            PropertyDefault::Rect {
                x,
                y,
                width,
                height,
            } => OwnedDefault::Rect([x, y, width, height]),
        }
    }
}

impl OwnedDefault {
    /// Pointer to the value in the FFI layout, or null for "use type default".
    fn as_ffi_ptr(&self) -> *const c_void {
        match self {
            OwnedDefault::None => ptr::null(),
            OwnedDefault::Int32(v) => (v as *const i32).cast(),
            OwnedDefault::Float(v) => (v as *const f32).cast(),
            OwnedDefault::Double(v) => (v as *const f64).cast(),
            OwnedDefault::Bool(v) => (v as *const bool).cast(),
            OwnedDefault::String(c) => match c {
                Some(cs) => {
                    // FFI expects const char* const* — pointer to a c-string pointer.
                    // We need stable storage; Box<*const c_char> would work but
                    // we sidestep by writing the pointer into a slot the caller
                    // owns. Here, return the pointer to the inner pointer of
                    // the CString box.
                    let p: *const i8 = cs.as_ptr();
                    // SAFETY: we leak this slot for the duration of the
                    // registration call. ClassBuilder::register takes &props
                    // by ref, so &p is valid for the call site only — the
                    // pointer is dereferenced synchronously by the C++ side.
                    // The returned pointer is to a stack temporary; we
                    // sidestep by NOT supporting String defaults via the
                    // borrow path. Use SetValue from Rust after construction.
                    // Returning null causes the C++ default ("") to apply.
                    let _ = p;
                    ptr::null()
                }
                None => ptr::null(),
            },
            OwnedDefault::Thickness(arr) | OwnedDefault::Color(arr) | OwnedDefault::Rect(arr) => {
                arr.as_ptr().cast()
            }
        }
    }
}

/// RAII handle for a registered class. Drop unregisters the class —
/// preventing new instances from being created — but the underlying
/// ClassData (and the boxed handler) survive as long as instances remain
/// alive. The intrusive refcount on the C++ side guarantees the handler
/// outlives any property-change callback fired during instance destruction.
pub struct ClassRegistration {
    token: NonNull<c_void>,
    _name: CString,
    num_props: u32,
}

// SAFETY: the Boxed handler is `Send`; the C++ side only touches the token
// via the C ABI surface, which is thread-safe per the registry mutex.
unsafe impl Send for ClassRegistration {}
unsafe impl Sync for ClassRegistration {}

impl ClassRegistration {
    /// Number of dependency properties registered against this class.
    #[must_use]
    pub fn num_properties(&self) -> u32 {
        self.num_props
    }

    /// Internal token (a `void*` to the C++-side `ClassData`). Used by
    /// dm_noesis_bevy when collecting registrations into the render-app sync.
    pub fn token(&self) -> NonNull<c_void> {
        self.token
    }
}

impl Drop for ClassRegistration {
    fn drop(&mut self) {
        // The C++ side owns the boxed handler (transferred at register
        // time) and is responsible for calling `class_handler_free_trampoline`
        // exactly once when the underlying ClassData is finally freed.
        // That happens *here* if no instances are alive, or deferred to
        // the last instance's destruction otherwise — which is the whole
        // point of the refcount: instances may legally outlive the Rust
        // `ClassRegistration` (e.g. when Bevy drops the registry resource
        // before the View tearing down).
        //
        // SAFETY: `self.token` was produced by `ClassBuilder::register`
        // and is freed exactly once here.
        unsafe { dm_noesis_class_unregister(self.token.as_ptr()) };
    }
}

/// Stable pointer to a Rust-backed instance, as observed by the
/// [`PropertyChangeHandler`] callback. Use it to drive
/// [`Instance::set_*`] / [`Instance::get_*`] without holding a Noesis ref
/// — the instance is owned by the visual tree.
#[derive(Copy, Clone, Debug)]
pub struct Instance(NonNull<c_void>);

impl Instance {
    /// Construct from a raw pointer received via the FFI callback.
    ///
    /// # Safety
    ///
    /// `ptr` must be a non-null pointer obtained from the FFI's
    /// property-change callback or from another [`Instance`] — it is treated
    /// as opaque and stays valid for the instance's lifetime.
    pub unsafe fn from_raw(ptr: NonNull<c_void>) -> Self {
        Self(ptr)
    }

    pub fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Set an `Int32` DP. Triggers the change callback if the value differs.
    pub fn set_int32(self, prop_index: u32, value: i32) {
        unsafe {
            dm_noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            );
        }
    }
    pub fn set_float(self, prop_index: u32, value: f32) {
        unsafe {
            dm_noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f32).cast(),
            );
        }
    }
    pub fn set_double(self, prop_index: u32, value: f64) {
        unsafe {
            dm_noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f64).cast(),
            );
        }
    }
    pub fn set_bool(self, prop_index: u32, value: bool) {
        unsafe {
            dm_noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const bool).cast(),
            );
        }
    }
    pub fn set_string(self, prop_index: u32, value: &str) {
        let cstr = CString::new(value).expect("string contained NUL");
        let ptr: *const i8 = cstr.as_ptr();
        unsafe {
            dm_noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&ptr as *const *const i8).cast(),
            );
        }
    }
    pub fn set_thickness(self, prop_index: u32, left: f32, top: f32, right: f32, bottom: f32) {
        let arr = [left, top, right, bottom];
        unsafe {
            dm_noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    pub fn set_color(self, prop_index: u32, r: f32, g: f32, b: f32, a: f32) {
        let arr = [r, g, b, a];
        unsafe {
            dm_noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    pub fn set_rect(self, prop_index: u32, x: f32, y: f32, width: f32, height: f32) {
        let arr = [x, y, width, height];
        unsafe {
            dm_noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }

    /// Read back an `Int32` DP. Returns `None` on bad input
    /// (instance pointer / index mismatch).
    pub fn get_int32(self, prop_index: u32) -> Option<i32> {
        let mut out: i32 = 0;
        let ok = unsafe {
            dm_noesis_instance_get_property(
                self.0.as_ptr(),
                prop_index,
                (&mut out as *mut i32).cast(),
            )
        };
        ok.then_some(out)
    }
    pub fn get_float(self, prop_index: u32) -> Option<f32> {
        let mut out: f32 = 0.0;
        let ok = unsafe {
            dm_noesis_instance_get_property(
                self.0.as_ptr(),
                prop_index,
                (&mut out as *mut f32).cast(),
            )
        };
        ok.then_some(out)
    }
    pub fn get_thickness(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            dm_noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    pub fn get_rect(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            dm_noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    /// Read back a `Color` DP as `(r, g, b, a)` floats in 0..=1. Returns
    /// `None` on bad input (instance pointer / index mismatch / type
    /// mismatch).
    pub fn get_color(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            dm_noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }

    /// Read the intrinsic size of an `ImageSource`-typed property's current
    /// value. Returns `None` when the source is null, not an
    /// `ImageSource` subclass, or the property index doesn't match an
    /// `ImageSource` property. Safe wrapper over [`image_source_size`] —
    /// useful for custom-control handlers (NineSlicer / ThreeSlicer) that
    /// need source dimensions without dropping into `unsafe`.
    #[must_use]
    pub fn get_image_source_size(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut raw_ptr: *mut c_void = ptr::null_mut();
        let ok = unsafe {
            dm_noesis_instance_get_property(self.0.as_ptr(), prop_index, (&raw mut raw_ptr).cast())
        };
        if !ok {
            return None;
        }
        let ptr = NonNull::new(raw_ptr)?;
        unsafe { image_source_size(ptr) }
    }
}

// SAFETY: Instance is just a raw pointer; the underlying object is owned by
// Noesis's visual tree and is safe to reference from any thread that respects
// the View's main-thread invariant.
unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

// ── Trampoline ─────────────────────────────────────────────────────────────

unsafe extern "C" fn prop_changed_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    value_ptr: *const c_void,
) {
    let handler = &mut *userdata.cast::<Box<dyn PropertyChangeHandler>>();
    let Some(instance) = NonNull::new(instance) else {
        return;
    };

    // We need the prop type to decode the value. The C++ side knows the type
    // tag for the prop but doesn't pass it across the FFI on the changed
    // callback (to keep the surface narrow). We recover it via a side table
    // populated at registration: see `with_class_props`.
    let value = decode_value(userdata, prop_index, value_ptr);
    handler.on_changed(Instance(instance), prop_index, value);
}

// Side table from (handler userdata pointer) → property type list, populated
// during ClassBuilder::register. We use the userdata pointer as the key
// because it's stable per-class and unique (one Box per registration).
//
// This avoids broadening the FFI callback signature — the C++ side already
// knows the prop type internally; the Rust side mirrors the list so it can
// decode `value_ptr` at the boundary.
static CLASS_PROP_TYPES: Mutex<Vec<(usize, Vec<PropType>)>> = Mutex::new(Vec::new());

fn record_prop_types(userdata: *mut c_void, types: Vec<PropType>) {
    let key = userdata as usize;
    let mut table = CLASS_PROP_TYPES.lock().expect("CLASS_PROP_TYPES poisoned");
    if let Some(slot) = table.iter_mut().find(|(k, _)| *k == key) {
        slot.1 = types;
    } else {
        table.push((key, types));
    }
}

fn forget_prop_types(userdata: *mut c_void) {
    let key = userdata as usize;
    let mut table = CLASS_PROP_TYPES.lock().expect("CLASS_PROP_TYPES poisoned");
    table.retain(|(k, _)| *k != key);
}

fn lookup_prop_type(userdata: *mut c_void, prop_index: u32) -> Option<PropType> {
    let key = userdata as usize;
    let table = CLASS_PROP_TYPES.lock().expect("CLASS_PROP_TYPES poisoned");
    table
        .iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, types)| types.get(prop_index as usize).copied())
}

unsafe fn decode_value<'a>(
    userdata: *mut c_void,
    prop_index: u32,
    value_ptr: *const c_void,
) -> PropertyValue<'a> {
    let kind = match lookup_prop_type(userdata, prop_index) {
        Some(k) => k,
        None => return PropertyValue::Bool(false), // unknown — defensive
    };
    if value_ptr.is_null() {
        return match kind {
            PropType::String => PropertyValue::String(None),
            PropType::ImageSource => PropertyValue::ImageSource(None),
            PropType::BaseComponent => PropertyValue::BaseComponent(None),
            PropType::Int32 => PropertyValue::Int32(0),
            PropType::Float => PropertyValue::Float(0.0),
            PropType::Double => PropertyValue::Double(0.0),
            PropType::Bool => PropertyValue::Bool(false),
            PropType::Thickness => PropertyValue::Thickness {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
            PropType::Color => PropertyValue::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            PropType::Rect => PropertyValue::Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };
    }
    match kind {
        PropType::Int32 => PropertyValue::Int32(*value_ptr.cast::<i32>()),
        PropType::Float => PropertyValue::Float(*value_ptr.cast::<f32>()),
        PropType::Double => PropertyValue::Double(*value_ptr.cast::<f64>()),
        PropType::Bool => PropertyValue::Bool(*value_ptr.cast::<bool>()),
        PropType::String => {
            // C++ passes &(const char*); deref to the c-string.
            let p = *value_ptr.cast::<*const i8>();
            let s = if p.is_null() {
                None
            } else {
                CStr::from_ptr(p).to_str().ok()
            };
            PropertyValue::String(s)
        }
        PropType::Thickness => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Thickness {
                left: *f,
                top: *f.add(1),
                right: *f.add(2),
                bottom: *f.add(3),
            }
        }
        PropType::Color => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Color {
                r: *f,
                g: *f.add(1),
                b: *f.add(2),
                a: *f.add(3),
            }
        }
        PropType::Rect => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Rect {
                x: *f,
                y: *f.add(1),
                width: *f.add(2),
                height: *f.add(3),
            }
        }
        PropType::ImageSource => {
            let p = *value_ptr.cast::<*mut c_void>();
            PropertyValue::ImageSource(NonNull::new(p))
        }
        PropType::BaseComponent => {
            let p = *value_ptr.cast::<*mut c_void>();
            PropertyValue::BaseComponent(NonNull::new(p))
        }
    }
}
