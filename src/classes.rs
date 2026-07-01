//! Register Rust-backed XAML classes.
//!
//! This is the Rust analogue of what the Noesis C# / Unity binding does for
//! managed code: it lets you declare new `<myns:Foo>` types whose dependency
//! properties + property-change logic live entirely in Rust. The C++ side
//! synthesizes a `Noesis::TypeClassBuilder` per consumer-named class and
//! installs a Factory creator that returns a per-base trampoline subclass
//! (`ClassBase::ContentControl` is the v1 base; sibling bases plug in
//! incrementally).
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
//!    [`Instance::set_thickness`] / etc., useful for "computed" properties
//!    (`NineSlicer`'s `TopLeftViewbox` family is the motivating example).
//! 5. Drop the [`ClassRegistration`] AFTER all live instances are released
//!    (typically at process shutdown). RAII + the `Send`/`Sync` bounds are
//!    deliberately conservative; registrations are cheap and rare.
//!
//! # Threading
//!
//! Property-changed callbacks fire on whatever thread drives the View, in
//! practice the main thread. The handler is stored behind a `Send` trait
//! bound; if you need cross-thread fan-out (e.g. Bevy ECS), keep the handler
//! body small and route to a channel / queue.
//!
//! # Re-entrancy
//!
//! [`Instance`] `set_*` calls fire the property-changed callback synchronously
//! if the new value differs. Guard against re-entrancy in the handler if you
//! plan to write back to the same property.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ffi::{CStr, c_char};
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};
use std::sync::Mutex;

use crate::drawing::DrawingContext;
use crate::ffi::{
    ClassBase, LayoutVtable, PropType, noesis_base_component_release, noesis_class_create_instance,
    noesis_class_register, noesis_class_register_enum_property, noesis_class_register_property_ex,
    noesis_class_set_coerce, noesis_class_set_layout, noesis_class_set_render,
    noesis_class_unregister, noesis_freezable_can_freeze, noesis_freezable_freeze,
    noesis_freezable_is_frozen, noesis_image_source_get_size, noesis_instance_get_property,
    noesis_instance_set_property, noesis_instance_set_readonly_property, noesis_uielement_arrange,
    noesis_uielement_desired_size, noesis_uielement_measure, noesis_visual_child,
    noesis_visual_children_count,
};

/// Free trampoline matching [`crate::ffi::ClassFreeFn`]. The C++ side holds a
/// pointer to this function and invokes it exactly once when the underlying
/// `ClassData` is finally freed (immediately at unregister if no instances
/// exist, or deferred to the last live instance's destruction). Drops the
/// double-boxed `Box<dyn PropertyChangeHandler>` whose ownership was
/// transferred to C++ at registration time, and clears the prop-types
/// scratch slot keyed on the same userdata pointer.
unsafe extern "C" fn class_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        forget_prop_types(userdata);
        // SAFETY: `userdata` is `Box::into_raw(Box<Box<dyn PropertyChangeHandler>>)`
        // produced by `ClassBuilder::register`. The C++ ClassData holds the
        // unique ownership; this is the matching `Box::from_raw` that ends it.
        unsafe {
            drop(Box::from_raw(
                userdata.cast::<Box<dyn PropertyChangeHandler>>(),
            ))
        };
    })
}

/// Read width / height of an `ImageSource` value (or any `BaseComponent*`
/// whose runtime type is an `ImageSource` subclass). Returns `None` when
/// the pointer is null or doesn't downcast.
///
/// Useful when a custom-control [`PropertyChangeHandler`] needs the source
/// dimensions to compute derived properties. `NineSlicer` / `ThreeSlicer`'s
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
    let ok = noesis_image_source_get_size(image_source.as_ptr(), &mut w, &mut h);
    ok.then_some((w, h))
}

/// Per-instance Rust callback. Implementations receive a stable instance
/// pointer (see [`Instance`]) and the index of the changed property. The index
/// matches the order in which DPs were added to the class.
///
/// # Re-entrancy
///
/// `on_changed` takes `&self`, not `&mut self`, because it is *re-entrant*: a
/// single handler box is shared by every instance of the class, and the
/// documented "computed property" pattern has a handler write *another* DP from
/// inside `on_changed` (e.g. `SliceThickness` changes → recompute viewboxes →
/// `instance.set_rect(...)`). That synchronous write re-enters Noesis, which
/// re-invokes this same handler before the outer call has returned. Holding a
/// `&mut self` across the user callback would alias on re-entry: undefined
/// behaviour. Handlers that need mutable state must use interior mutability
/// (`Cell` / `RefCell` / `Mutex` / atomics); re-entering a `RefCell` borrow is a
/// controlled panic, never UB.
pub trait PropertyChangeHandler: Send + 'static {
    fn on_changed(&self, instance: Instance, prop_index: u32, value: PropertyValue<'_>);
}

/// Property value as observed by the change callback. Variant matches the
/// [`PropType`] declared at registration time.
///
/// Borrowed variants (`String`, `ImageSource`, `BaseComponent`) reference
/// Noesis-owned storage that may be invalidated by the next layout pass.
/// Copy if you need to keep the value past the callback.
#[derive(Debug)]
pub enum PropertyValue<'a> {
    Int32(i32),
    UInt32(u32),
    /// A `uint64` DP value (e.g. a packed row identity).
    UInt64(u64),
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
    /// `Noesis::Point` (x, y).
    Point {
        x: f32,
        y: f32,
    },
    /// `Noesis::Size` (width, height).
    Size {
        width: f32,
        height: f32,
    },
    /// `Noesis::Vector2` (x, y).
    Vector {
        x: f32,
        y: f32,
    },
    /// Runtime-enum-typed DP value (the underlying `int32` member value).
    Enum(i32),
    /// Borrowed `Noesis::ImageSource*` (or null). Treat as opaque.
    ImageSource(Option<NonNull<c_void>>),
    /// Borrowed `Noesis::BaseComponent*` (or null). Treat as opaque.
    BaseComponent(Option<NonNull<c_void>>),
}

/// One registered dependency property + its metadata options.
struct PropSpec {
    name: CString,
    kind: PropType,
    default: OwnedDefault,
    options: PropertyOptions,
    /// For [`PropType::Enum`] DPs: the reflected name of the runtime enum
    /// (registered via [`crate::reflection::register_enum`]). `None` for all
    /// other property types.
    enum_type: Option<CString>,
}

/// Builder for a single class registration.
pub struct ClassBuilder<H: PropertyChangeHandler> {
    name: CString,
    base: ClassBase,
    handler: H,
    props: Vec<PropSpec>,
    coerce: Option<Box<dyn CoerceHandler>>,
    layout: Option<Box<dyn LayoutHandler>>,
    render: Option<Box<dyn RenderHandler>>,
}

enum OwnedDefault {
    None,
    Int32(i32),
    UInt32(u32),
    UInt64(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(Option<StringDefault>),
    Thickness([f32; 4]),
    Color([f32; 4]),
    Rect([f32; 4]),
    Point([f32; 2]),
    Size([f32; 2]),
    Vector([f32; 2]),
    Enum(i32),
}

/// Owned storage for a `NOESIS_PROP_STRING` default. The FFI expects a
/// `const char* const*` (a pointer *to* a c-string pointer), so we keep both
/// the NUL-terminated bytes (`_bytes`) and a stable slot (`ptr`) holding the
/// pointer into them. `ptr` is computed from the heap-allocated `CString`
/// buffer, which is move-stable, so `&self.ptr` stays valid for the synchronous
/// registration call regardless of where the enclosing `PropSpec` lives.
struct StringDefault {
    /// Owns the bytes that `ptr` points into; never read directly.
    _bytes: CString,
    /// `_bytes.as_ptr()`, stored so its address can be handed to the FFI.
    ptr: *const c_char,
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
            coerce: None,
            layout: None,
            render: None,
        }
    }

    /// Append a dependency property initialized to its type's default value.
    /// The returned `u32` is the dense index the change callback receives for
    /// this property; indices grow in addition order starting at 0. Use
    /// [`Self::add_property_with`] to supply an explicit default.
    pub fn add_property(&mut self, name: &str, kind: PropType) -> u32 {
        self.add_property_with(name, kind, PropertyDefault::None)
    }

    /// Same as [`Self::add_property`] but with an explicit default value.
    /// `ImageSource` and `BaseComponent` properties have no default variant and
    /// always start null.
    pub fn add_property_with(
        &mut self,
        name: &str,
        kind: PropType,
        default: PropertyDefault<'_>,
    ) -> u32 {
        self.add_property_ex(name, kind, default, PropertyOptions::default())
    }

    /// Append a dependency property with richer metadata
    /// ([`PropertyOptions`]): `FrameworkPropertyMetadataOptions` (e.g.
    /// [`fpm_options::AFFECTS_MEASURE`]), a read-only access flag, and/or
    /// opt-in coercion. Coercion requires a handler installed via
    /// [`Self::set_coerce`] and only applies to scalar / `Thickness` / `Color`
    /// / `Rect` / `Point` / `Size` / `Vector` properties (the first 32
    /// properties of a class; enum / object / string tags are not coercible).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn add_property_ex(
        &mut self,
        name: &str,
        kind: PropType,
        default: PropertyDefault<'_>,
        options: PropertyOptions,
    ) -> u32 {
        let cstr = CString::new(name).expect("property name contained NUL");
        self.props.push(PropSpec {
            name: cstr,
            kind,
            default: default.into_owned(),
            options,
            enum_type: None,
        });
        self.props.len() as u32 - 1
    }

    /// Append a dependency property whose value type is a runtime enum
    /// (registered with [`crate::reflection::register_enum`]). The DP stores an
    /// `int32` but reports the enum as its reflected type, so XAML enum-string
    /// parsing, the `EnumConverter`, and `Style` setters resolve it. `default`
    /// is the initial member value. Coercion is not offered for enum DPs.
    ///
    /// Returns the dense property index. The enum type must already be
    /// registered when the class is [`Self::register`]ed, or registration of
    /// this property fails (and [`Self::register`] returns `None`).
    ///
    /// # Panics
    ///
    /// Panics if `name` / `enum_type_name` contain an interior NUL.
    pub fn add_enum_property(
        &mut self,
        name: &str,
        enum_type_name: &str,
        default: i32,
        options: PropertyOptions,
    ) -> u32 {
        let cstr = CString::new(name).expect("property name contained NUL");
        let etype = CString::new(enum_type_name).expect("enum type name contained NUL");
        self.props.push(PropSpec {
            name: cstr,
            kind: PropType::Enum,
            default: OwnedDefault::Enum(default),
            options,
            enum_type: Some(etype),
        });
        self.props.len() as u32 - 1
    }

    /// Install a class-level coerce handler. Individual properties opt in by
    /// passing [`PropertyOptions::coerce`] `= true` to [`Self::add_property_ex`].
    /// The handler's [`CoerceHandler::coerce`] runs inside Noesis's value
    /// pipeline whenever a coerced property's effective value is computed, and
    /// returns the clamped / transformed result (or [`Coerced::Unchanged`] to
    /// pass the value through).
    pub fn set_coerce(&mut self, handler: impl CoerceHandler) {
        self.coerce = Some(Box::new(handler));
    }

    /// Install a layout handler so the class participates in the layout system
    /// via `MeasureOverride` / `ArrangeOverride`. Without a handler the base
    /// class's default layout runs. See [`LayoutHandler`].
    pub fn set_layout(&mut self, handler: impl LayoutHandler) {
        self.layout = Some(Box::new(handler));
    }

    /// Install a render handler so the class draws immediate-mode content via
    /// `OnRender`. Without a handler the base class renders normally. The
    /// handler's [`RenderHandler::render`] receives a borrowed
    /// [`DrawingContext`] for the duration of the call; issue draw / push / pop
    /// commands through it. See [`RenderHandler`].
    pub fn set_render(&mut self, handler: impl RenderHandler) {
        self.render = Some(Box::new(handler));
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
            coerce,
            layout,
            render,
        } = self;
        let prop_types: Vec<PropType> = props.iter().map(|p| p.kind).collect();

        // Box twice so we have a stable thin pointer for the C ABI userdata,
        // matching the pattern in `events::subscribe_click`.
        let boxed: Box<Box<dyn PropertyChangeHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // Record the prop type list BEFORE the FFI call so the trampoline
        // can decode `value_ptr` if the C++ side fires a callback during
        // registration (e.g. on a default-value initialization).
        record_prop_types(userdata.cast(), prop_types.clone());

        let token = unsafe {
            noesis_class_register(
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

        for spec in &props {
            let idx = if let Some(enum_type) = &spec.enum_type {
                // Enum DPs need the runtime TypeEnum bound at registration; they
                // go through the dedicated entry point (coercion not offered).
                let default = match spec.default {
                    OwnedDefault::Enum(v) => v,
                    _ => 0,
                };
                unsafe {
                    noesis_class_register_enum_property(
                        token.as_ptr(),
                        spec.name.as_ptr(),
                        enum_type.as_ptr(),
                        default,
                        spec.options.fpm_options,
                        spec.options.read_only,
                    )
                }
            } else {
                let default_ptr = spec.default.as_ffi_ptr();
                unsafe {
                    noesis_class_register_property_ex(
                        token.as_ptr(),
                        spec.name.as_ptr(),
                        spec.kind,
                        default_ptr,
                        spec.options.fpm_options,
                        spec.options.read_only,
                        spec.options.coerce,
                    )
                }
            };
            if idx == u32::MAX {
                // C++ owns the property-handler box now; unregister triggers
                // its free trampoline (no instances exist yet). The coerce /
                // layout boxes were never donated, so drop them here.
                unsafe { noesis_class_unregister(token.as_ptr()) };
                drop(coerce);
                drop(layout);
                drop(render);
                return None;
            }
        }

        // Donate the coerce handler (if any). Ownership transfers to the C++
        // ClassData, freed via `coerce_handler_free_trampoline` at teardown.
        if let Some(handler) = coerce {
            let boxed: Box<Box<dyn CoerceHandler>> = Box::new(handler);
            let coerce_ud = Box::into_raw(boxed);
            // The coerce trampoline decodes `value_ptr` via the same side
            // table; key it on the coerce userdata pointer.
            record_prop_types(coerce_ud.cast(), prop_types);
            unsafe {
                noesis_class_set_coerce(
                    token.as_ptr(),
                    coerce_trampoline,
                    coerce_ud.cast(),
                    coerce_handler_free_trampoline,
                );
            }
        }

        if let Some(handler) = layout {
            let boxed: Box<Box<dyn LayoutHandler>> = Box::new(handler);
            let layout_ud = Box::into_raw(boxed);
            let vtable = LayoutVtable {
                measure: Some(layout_measure_trampoline),
                arrange: Some(layout_arrange_trampoline),
            };
            unsafe {
                noesis_class_set_layout(
                    token.as_ptr(),
                    &vtable,
                    layout_ud.cast(),
                    layout_handler_free_trampoline,
                );
            }
        }

        if let Some(handler) = render {
            let boxed: Box<Box<dyn RenderHandler>> = Box::new(handler);
            let render_ud = Box::into_raw(boxed);
            unsafe {
                noesis_class_set_render(
                    token.as_ptr(),
                    render_trampoline,
                    render_ud.cast(),
                    render_handler_free_trampoline,
                );
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
    UInt32(u32),
    UInt64(u64),
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
    Point {
        x: f32,
        y: f32,
    },
    Size {
        width: f32,
        height: f32,
    },
    Vector {
        x: f32,
        y: f32,
    },
    /// Default member value for a runtime-enum-typed DP (set the enum int
    /// directly, e.g. via [`crate::reflection::EnumType::value_from_name`]).
    Enum(i32),
}

impl PropertyDefault<'_> {
    fn into_owned(self) -> OwnedDefault {
        match self {
            PropertyDefault::None => OwnedDefault::None,
            PropertyDefault::Int32(v) => OwnedDefault::Int32(v),
            PropertyDefault::UInt32(v) => OwnedDefault::UInt32(v),
            PropertyDefault::UInt64(v) => OwnedDefault::UInt64(v),
            PropertyDefault::Float(v) => OwnedDefault::Float(v),
            PropertyDefault::Double(v) => OwnedDefault::Double(v),
            PropertyDefault::Bool(v) => OwnedDefault::Bool(v),
            PropertyDefault::String(s) => {
                let slot = CString::new(s).ok().map(|bytes| {
                    let ptr = bytes.as_ptr();
                    StringDefault { _bytes: bytes, ptr }
                });
                OwnedDefault::String(slot)
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
            PropertyDefault::Point { x, y } => OwnedDefault::Point([x, y]),
            PropertyDefault::Size { width, height } => OwnedDefault::Size([width, height]),
            PropertyDefault::Vector { x, y } => OwnedDefault::Vector([x, y]),
            PropertyDefault::Enum(v) => OwnedDefault::Enum(v),
        }
    }
}

impl OwnedDefault {
    /// Pointer to the value in the FFI layout, or null for "use type default".
    fn as_ffi_ptr(&self) -> *const c_void {
        match self {
            OwnedDefault::None => ptr::null(),
            OwnedDefault::Int32(v) => (v as *const i32).cast(),
            OwnedDefault::UInt32(v) => (v as *const u32).cast(),
            OwnedDefault::UInt64(v) => (v as *const u64).cast(),
            OwnedDefault::Float(v) => (v as *const f32).cast(),
            OwnedDefault::Double(v) => (v as *const f64).cast(),
            OwnedDefault::Bool(v) => (v as *const bool).cast(),
            OwnedDefault::String(slot) => match slot {
                // FFI expects `const char* const*`, a pointer to a c-string
                // pointer. `slot.ptr` is that c-string pointer, held in stable
                // storage owned by this `OwnedDefault`; we hand the FFI the
                // address of that slot. It is dereferenced synchronously by the
                // C++ side during the registration call, while `self` is alive.
                Some(slot) => (&slot.ptr as *const *const c_char).cast(),
                // Interior NUL (or no default): fall back to the C++ "" default.
                None => ptr::null(),
            },
            OwnedDefault::Thickness(arr) | OwnedDefault::Color(arr) | OwnedDefault::Rect(arr) => {
                arr.as_ptr().cast()
            }
            OwnedDefault::Point(arr) | OwnedDefault::Size(arr) | OwnedDefault::Vector(arr) => {
                arr.as_ptr().cast()
            }
            OwnedDefault::Enum(v) => (v as *const i32).cast(),
        }
    }
}

/// RAII handle for a registered class. Drop unregisters the class
/// (preventing new instances from being created), but the underlying
/// `ClassData` (and the boxed handler) survive as long as instances remain
/// alive. The intrusive refcount on the C++ side guarantees the handler
/// outlives any property-change callback fired during instance destruction.
#[must_use = "dropping the guard immediately clears the registration"]
pub struct ClassRegistration {
    token: NonNull<c_void>,
    _name: CString,
    num_props: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClassRegistration {}

impl ClassRegistration {
    /// Number of dependency properties registered against this class.
    #[must_use]
    pub fn num_properties(&self) -> u32 {
        self.num_props
    }

    /// Internal token (a `void*` to the C++-side `ClassData`). Used by
    /// `noesis_bevy` when collecting registrations into the render-app sync.
    pub fn token(&self) -> NonNull<c_void> {
        self.token
    }

    /// Instantiate this class directly from Rust, without a XAML reference.
    /// Returns `None` only if the C++ side rejected the token (it never should
    /// for a live registration).
    ///
    /// The instance is a `DependencyObject` carrying this class's registered
    /// DPs, which makes it a ready-made data-binding source: set it as an
    /// element's `DataContext`
    /// ([`FrameworkElement::set_data_context`](crate::view::FrameworkElement::set_data_context))
    /// and author `{Binding SomeDP}` in XAML. Writing a DP from Rust via the
    /// returned [`ClassInstance`]'s [`Instance`] handle raises the change
    /// notification the binding engine observes, so the bound element updates
    /// on the next `View::update`.
    ///
    /// The registration must outlive every [`ClassInstance`] it produces (the
    /// same rule the C++ refcount enforces for XAML-created instances).
    #[must_use]
    pub fn create_instance(&self) -> Option<ClassInstance> {
        // SAFETY: `self.token` is a live ClassData* for the lifetime of `self`.
        let ptr = unsafe { noesis_class_create_instance(self.token.as_ptr()) };
        NonNull::new(ptr).map(|ptr| ClassInstance { ptr })
    }
}

/// An owned instance of a Rust-backed class created via
/// [`ClassRegistration::create_instance`]. Holds a `+1` reference on the
/// underlying object and releases it on drop. Most useful as a binding-source
/// view model (set it as a `DataContext`).
pub struct ClassInstance {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClassInstance {}

impl ClassInstance {
    /// A non-owning [`Instance`] handle for driving the DPs (`set_*` / `get_*`).
    /// The returned handle borrows this object; keep `self` alive while using it.
    #[must_use]
    pub fn handle(&self) -> Instance {
        // SAFETY: self.ptr is a live instance pointer for the lifetime of self.
        unsafe { Instance::from_raw(self.ptr) }
    }

    /// Raw `Noesis::BaseComponent*`, for handing to APIs that take one (e.g.
    /// `set_data_context`). Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Freeze this instance, if it is a [`ClassBase::Freezable`]-based class
    /// (`Noesis::Freezable::Freeze`). After freezing, the object is immutable
    /// and [`Self::is_frozen`] reads back `true`. Returns `false` if the object
    /// is not a `Freezable` or cannot currently be frozen.
    pub fn freeze(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_freezable_freeze(self.ptr.as_ptr()) }
    }

    /// Whether this instance is currently frozen (`Noesis::Freezable::IsFrozen`).
    /// Always `false` for non-`Freezable` classes.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_freezable_is_frozen(self.ptr.as_ptr()) }
    }

    /// Whether this instance can be frozen (`Noesis::Freezable::CanFreeze`).
    /// Always `false` for non-`Freezable` classes.
    #[must_use]
    pub fn can_freeze(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_freezable_can_freeze(self.ptr.as_ptr()) }
    }
}

impl Drop for ClassInstance {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_class_create_instance with +1 ref.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

impl Drop for ClassRegistration {
    fn drop(&mut self) {
        // The C++ side owns the boxed handler (transferred at register
        // time) and is responsible for calling `class_handler_free_trampoline`
        // exactly once when the underlying ClassData is finally freed.
        // That happens *here* if no instances are alive, or deferred to
        // the last instance's destruction otherwise, which is the whole
        // point of the refcount: instances may legally outlive the Rust
        // `ClassRegistration` (e.g. when Bevy drops the registry resource
        // before the View tearing down).
        //
        // SAFETY: `self.token` was produced by `ClassBuilder::register`
        // and is freed exactly once here.
        unsafe { noesis_class_unregister(self.token.as_ptr()) };
    }
}

/// Stable pointer to a Rust-backed instance, as observed by the
/// [`PropertyChangeHandler`] callback. Use it to drive the
/// [`Instance`] `set_*` / `get_*` methods without holding a Noesis ref:
/// the instance is owned by the visual tree.
#[derive(Copy, Clone, Debug)]
pub struct Instance(NonNull<c_void>);

impl Instance {
    /// Construct from a raw pointer received via the FFI callback.
    ///
    /// # Safety
    ///
    /// `ptr` must be a non-null pointer obtained from the FFI's
    /// property-change callback or from another [`Instance`]. It is treated
    /// as opaque and stays valid for the instance's lifetime.
    pub unsafe fn from_raw(ptr: NonNull<c_void>) -> Self {
        Self(ptr)
    }

    /// Raw opaque instance pointer, for FFI calls not yet wrapped here.
    pub fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Set an `Int32` DP. Triggers the change callback if the value differs.
    pub fn set_int32(self, prop_index: u32, value: i32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            );
        }
    }
    /// Set a `UInt64` DP. Triggers the change callback if the value differs.
    /// Register the DP with [`PropType::UInt64`]. The motivating use is stashing
    /// a stable row identity (e.g. a Bevy `Entity`'s 64-bit bits) on a bound row
    /// view model, so a per-row event handler can recover it off the event
    /// source's `DataContext`.
    pub fn set_u64(self, prop_index: u32, value: u64) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const u64).cast(),
            );
        }
    }
    /// Set a `Float` DP. Triggers the change callback if the value differs.
    pub fn set_float(self, prop_index: u32, value: f32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f32).cast(),
            );
        }
    }
    /// Set a `Double` DP. Triggers the change callback if the value differs.
    pub fn set_double(self, prop_index: u32, value: f64) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f64).cast(),
            );
        }
    }
    /// Set a `Bool` DP. Triggers the change callback if the value differs.
    pub fn set_bool(self, prop_index: u32, value: bool) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const bool).cast(),
            );
        }
    }
    /// Set a `String` DP. Triggers the change callback if the value differs.
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL byte.
    pub fn set_string(self, prop_index: u32, value: &str) {
        let cstr = CString::new(value).expect("string contained NUL");
        let ptr: *const c_char = cstr.as_ptr();
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&ptr as *const *const c_char).cast(),
            );
        }
    }
    /// Set a `Thickness` DP from its four edge widths, in device-independent
    /// pixels.
    pub fn set_thickness(self, prop_index: u32, left: f32, top: f32, right: f32, bottom: f32) {
        let arr = [left, top, right, bottom];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set a `Color` DP from RGBA components in 0..=1.
    pub fn set_color(self, prop_index: u32, r: f32, g: f32, b: f32, a: f32) {
        let arr = [r, g, b, a];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set a `Rect` DP from its origin and extent.
    pub fn set_rect(self, prop_index: u32, x: f32, y: f32, width: f32, height: f32) {
        let arr = [x, y, width, height];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set a `Point` DP (`Noesis::Point`).
    pub fn set_point(self, prop_index: u32, x: f32, y: f32) {
        let arr = [x, y];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set a `Size` DP (`Noesis::Size`).
    pub fn set_size(self, prop_index: u32, width: f32, height: f32) {
        let arr = [width, height];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set a `Vector` DP (`Noesis::Vector2`).
    pub fn set_vector(self, prop_index: u32, x: f32, y: f32) {
        let arr = [x, y];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Set an enum DP (the underlying `int32` member value). Register the DP
    /// with [`ClassBuilder::add_enum_property`].
    pub fn set_enum(self, prop_index: u32, value: i32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            );
        }
    }
    /// Set an `ImageSource` / `BaseComponent` DP to a borrowed
    /// `Noesis::BaseComponent*`. The C++ side stores its own reference, so the
    /// caller keeps ownership of `component` (pass `null` to clear). The
    /// motivating use is binding a control to a Rust-backed
    /// [`crate::commands::Command`]: register a `BaseComponent` DP on the view
    /// model, point it at `command.raw()`, then bind `Command="{Binding ...}"`.
    ///
    /// # Safety
    ///
    /// `component` must be null or a live `Noesis::BaseComponent*` (e.g. from
    /// [`crate::commands::Command::raw`] or [`ClassInstance::raw`]). The
    /// caller's reference is not consumed.
    pub unsafe fn set_component(self, prop_index: u32, component: *mut c_void) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&component as *const *mut c_void).cast(),
            );
        }
    }

    /// Assign a command (any [`AsCommand`](crate::commands::AsCommand): a
    /// [`Command`](crate::commands::Command),
    /// [`RoutedCommand`](crate::commands::RoutedCommand),
    /// [`RoutedUICommand`](crate::commands::RoutedUICommand), or built-in
    /// [`BorrowedCommand`](crate::commands::BorrowedCommand)) to a
    /// `BaseComponent` DP (register it with
    /// [`ClassBuilder::add_property`](crate::classes::ClassBuilder::add_property)
    /// and [`PropType::BaseComponent`]).
    /// The C++ side stores its own reference, so the caller keeps ownership of
    /// `command`.
    ///
    /// This is the safe, `unsafe`-free counterpart of
    /// [`set_component`](Self::set_component) for the command case: the
    /// `&impl AsCommand` borrow encodes the live-`BaseComponent` invariant. Set
    /// the instance as a `DataContext` and bind `Command="{Binding ThatProperty}"`
    /// in XAML. See the [`crate::commands`] module docs.
    pub fn set_command(self, prop_index: u32, command: &impl crate::commands::AsCommand) {
        // SAFETY: `command.command_ptr()` is a live ICommand* (a BaseComponent*
        // at runtime) borrowed for the duration of this synchronous call; the
        // DP stores its own reference, leaving the caller's ownership intact.
        unsafe { self.set_component(prop_index, command.command_ptr()) }
    }

    /// Read back an `Int32` DP. Returns `None` on bad input
    /// (instance pointer / index mismatch).
    pub fn get_int32(self, prop_index: u32) -> Option<i32> {
        let mut out: i32 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut i32).cast())
        };
        ok.then_some(out)
    }
    /// Read back a `UInt64` DP. Returns `None` on bad input
    /// (instance pointer / index mismatch).
    pub fn get_u64(self, prop_index: u32) -> Option<u64> {
        let mut out: u64 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut u64).cast())
        };
        ok.then_some(out)
    }
    /// Read back a `Float` DP. Returns `None` on bad input
    /// (instance pointer / index mismatch).
    pub fn get_float(self, prop_index: u32) -> Option<f32> {
        let mut out: f32 = 0.0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut f32).cast())
        };
        ok.then_some(out)
    }
    /// Read back a `String` DP. Returns `None` on bad input (instance pointer /
    /// index mismatch / type mismatch) or a null string pointer.
    pub fn get_string(self, prop_index: u32) -> Option<String> {
        let mut p: *const c_char = ptr::null();
        let ok = unsafe {
            noesis_instance_get_property(
                self.0.as_ptr(),
                prop_index,
                (&mut p as *mut *const c_char).cast(),
            )
        };
        if !ok || p.is_null() {
            return None;
        }
        // SAFETY: p is a live NUL-terminated UTF-8 string borrowed from
        // Noesis-owned storage while we hold our instance reference; copy out
        // before yielding control.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
    /// Read back a `Thickness` DP as `(left, top, right, bottom)`.
    pub fn get_thickness(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    /// Read back a `Rect` DP as `(x, y, width, height)`.
    pub fn get_rect(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    /// Read back a `Point` DP as `(x, y)`.
    pub fn get_point(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Read back a `Size` DP as `(width, height)`.
    pub fn get_size(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Read back a `Vector` DP as `(x, y)`.
    pub fn get_vector(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Read back an enum DP as its underlying `int32` member value.
    pub fn get_enum(self, prop_index: u32) -> Option<i32> {
        let mut out: i32 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut i32).cast())
        };
        ok.then_some(out)
    }
    /// Read back a `Color` DP as `(r, g, b, a)` floats in 0..=1. Returns
    /// `None` on bad input (instance pointer / index mismatch / type
    /// mismatch).
    pub fn get_color(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }

    /// Read the intrinsic size of an `ImageSource`-typed property's current
    /// value. Returns `None` when the source is null, not an
    /// `ImageSource` subclass, or the property index doesn't match an
    /// `ImageSource` property. Safe wrapper over [`image_source_size`] for
    /// custom-control handlers (`NineSlicer` / `ThreeSlicer`) that
    /// need source dimensions without dropping into `unsafe`.
    #[must_use]
    pub fn get_image_source_size(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut raw_ptr: *mut c_void = ptr::null_mut();
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&raw mut raw_ptr).cast())
        };
        if !ok {
            return None;
        }
        let ptr = NonNull::new(raw_ptr)?;
        unsafe { image_source_size(ptr) }
    }
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Instance {}

// ── Trampoline ─────────────────────────────────────────────────────────────

unsafe extern "C" fn prop_changed_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    value_ptr: *const c_void,
) {
    crate::panic_guard::guard(|| {
        // Shared `&`, never `&mut`: the handler is re-entrant (a `set_*` inside
        // `on_changed` re-invokes this trampoline with the same `userdata`
        // box). See `PropertyChangeHandler` docs.
        let handler = &*userdata.cast::<Box<dyn PropertyChangeHandler>>();
        let Some(instance) = NonNull::new(instance) else {
            return;
        };

        // We need the prop type to decode the value. The C++ side knows the type
        // tag for the prop but doesn't pass it across the FFI on the changed
        // callback (to keep the surface narrow). We recover it via a side table
        // populated at registration: see `with_class_props`.
        let value = decode_value(userdata, prop_index, value_ptr);
        handler.on_changed(Instance(instance), prop_index, value);
    })
}

// Side table from (handler userdata pointer) → property type list, populated
// during ClassBuilder::register. We use the userdata pointer as the key
// because it's stable per-class and unique (one Box per registration).
//
// This avoids broadening the FFI callback signature: the C++ side already
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
        None => return PropertyValue::Bool(false), // unknown; defensive
    };
    if value_ptr.is_null() {
        return match kind {
            PropType::String => PropertyValue::String(None),
            PropType::ImageSource => PropertyValue::ImageSource(None),
            PropType::BaseComponent => PropertyValue::BaseComponent(None),
            PropType::Int32 => PropertyValue::Int32(0),
            PropType::UInt32 => PropertyValue::UInt32(0),
            PropType::UInt64 => PropertyValue::UInt64(0),
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
            PropType::Point => PropertyValue::Point { x: 0.0, y: 0.0 },
            PropType::Size => PropertyValue::Size {
                width: 0.0,
                height: 0.0,
            },
            PropType::Vector => PropertyValue::Vector { x: 0.0, y: 0.0 },
            PropType::Enum => PropertyValue::Enum(0),
        };
    }
    match kind {
        PropType::Int32 => PropertyValue::Int32(*value_ptr.cast::<i32>()),
        PropType::UInt32 => PropertyValue::UInt32(*value_ptr.cast::<u32>()),
        PropType::UInt64 => PropertyValue::UInt64(*value_ptr.cast::<u64>()),
        PropType::Float => PropertyValue::Float(*value_ptr.cast::<f32>()),
        PropType::Double => PropertyValue::Double(*value_ptr.cast::<f64>()),
        PropType::Bool => PropertyValue::Bool(*value_ptr.cast::<bool>()),
        PropType::String => {
            // C++ passes &(const char*); deref to the c-string.
            let p = *value_ptr.cast::<*const c_char>();
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
        PropType::Point => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Point {
                x: *f,
                y: *f.add(1),
            }
        }
        PropType::Size => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Size {
                width: *f,
                height: *f.add(1),
            }
        }
        PropType::Vector => {
            let f = value_ptr.cast::<f32>();
            PropertyValue::Vector {
                x: *f,
                y: *f.add(1),
            }
        }
        PropType::Enum => PropertyValue::Enum(*value_ptr.cast::<i32>()),
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

/// `FrameworkPropertyMetadataOptions` bit flags (mirror of the Noesis enum in
/// `NsGui/FrameworkPropertyMetadata.h`). OR these together into
/// [`PropertyOptions::fpm_options`] so changing the property invalidates the
/// matching layout / render pass.
pub mod fpm_options {
    /// No framework options.
    pub const NONE: u32 = 0x000;
    /// A change re-runs the owning element's measure pass.
    pub const AFFECTS_MEASURE: u32 = 0x001;
    /// A change re-runs the owning element's arrange pass.
    pub const AFFECTS_ARRANGE: u32 = 0x002;
    /// A change re-runs the parent's measure pass.
    pub const AFFECTS_PARENT_MEASURE: u32 = 0x004;
    /// A change re-runs the parent's arrange pass.
    pub const AFFECTS_PARENT_ARRANGE: u32 = 0x008;
    /// A change re-runs the owning element's render pass.
    pub const AFFECTS_RENDER: u32 = 0x010;
    /// The property value is inherited down the logical tree.
    pub const INHERITS: u32 = 0x020;
}

/// Metadata options for [`ClassBuilder::add_property_ex`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PropertyOptions {
    /// Bitmask of [`fpm_options`] flags. Non-zero promotes the DP's metadata
    /// to a `FrameworkPropertyMetadata`.
    pub fpm_options: u32,
    /// Register the DP read-only: the ordinary setter paths (XAML, bindings,
    /// [`Instance::set_int32`] & friends) reject writes; only
    /// [`Instance::set_readonly_int32`] & friends can mutate it.
    pub read_only: bool,
    /// Route this DP through the class coerce handler ([`ClassBuilder::set_coerce`]).
    /// Only honored for scalar / `Thickness` / `Color` / `Rect` / `Point` /
    /// `Size` / `Vector` properties.
    pub coerce: bool,
}

/// A coerced value returned by [`CoerceHandler::coerce`]. The variant MUST
/// match the property's registered [`PropType`]; a mismatch is ignored (the
/// pre-coercion value passes through). Use [`Coerced::Unchanged`] to accept the
/// input as-is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coerced {
    /// Leave the value unchanged.
    Unchanged,
    Int32(i32),
    UInt32(u32),
    Float(f32),
    Double(f64),
    Bool(bool),
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
    Point {
        x: f32,
        y: f32,
    },
    Size {
        width: f32,
        height: f32,
    },
    Vector {
        x: f32,
        y: f32,
    },
}

/// Per-class coercion logic. Installed via [`ClassBuilder::set_coerce`]; runs
/// inside Noesis's value pipeline whenever a coerced property's effective value
/// is computed (e.g. on every `SetValue`). Return a clamped / transformed
/// value, or [`Coerced::Unchanged`] to pass it through. A no-op coerce yields
/// the input verbatim, so a clamp to `[0, 100]` that returns `100` for an input
/// of `999` is observable through a read-back.
///
/// Takes `&self` (re-entrant: coercion runs inside the value pipeline and a
/// handler that reads or writes other coerced DPs can re-enter this same
/// per-class handler box; use interior mutability for handler state).
pub trait CoerceHandler: Send + 'static {
    fn coerce(&self, instance: Instance, prop_index: u32, value: PropertyValue<'_>) -> Coerced;
}

/// A width/height pair in DIPs, used by [`LayoutHandler`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    /// The zero size.
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Custom layout participation, installed via [`ClassBuilder::set_layout`].
/// The trampoline subclass's `MeasureOverride` / `ArrangeOverride` forward
/// here. A handler that returns a fixed size makes a self-sizing element; to
/// lay out children, enumerate them with [`Instance::layout_child_count`] /
/// [`Instance::layout_child`] and call [`LayoutChild::measure`] /
/// [`LayoutChild::arrange`].
///
/// Default impls make the element take zero space (measure) and accept the
/// final size (arrange); override the half you need.
///
/// Methods take `&self` (re-entrant: a single handler box is shared by every
/// instance of the class, so a panel that lays out children of its own type
/// re-enters `measure`/`arrange` on the same box synchronously; use interior
/// mutability for handler state).
pub trait LayoutHandler: Send + 'static {
    /// Return the element's desired size given the available size.
    fn measure(&self, instance: Instance, available: Size) -> Size {
        let _ = (instance, available);
        Size::ZERO
    }

    /// Position children within `final_size` and return the size actually used.
    fn arrange(&self, instance: Instance, final_size: Size) -> Size {
        let _ = instance;
        final_size
    }
}

/// A borrowed child element handed to a [`LayoutHandler`] for measuring /
/// arranging. Valid only for the duration of the layout callback (it borrows a
/// Noesis-owned `UIElement*`; do not store it).
pub struct LayoutChild {
    ptr: NonNull<c_void>,
}

impl LayoutChild {
    /// Run the child's measure pass with the given available size. Returns
    /// `false` if the child is not a `UIElement`.
    pub fn measure(&self, available: Size) -> bool {
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        unsafe { noesis_uielement_measure(self.ptr.as_ptr(), available.width, available.height) }
    }

    /// Run the child's arrange pass at `(x, y)` with size `(w, h)` in this
    /// element's coordinate space. Returns `false` if not a `UIElement`.
    pub fn arrange(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        unsafe { noesis_uielement_arrange(self.ptr.as_ptr(), x, y, w, h) }
    }

    /// Read the child's `DesiredSize` (valid after [`Self::measure`]).
    #[must_use]
    pub fn desired_size(&self) -> Option<Size> {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        let ok = unsafe { noesis_uielement_desired_size(self.ptr.as_ptr(), &mut w, &mut h) };
        ok.then_some(Size::new(w, h))
    }

    /// Raw borrowed `Noesis::UIElement*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Instance {
    /// Number of visual children (for a custom [`LayoutHandler`]).
    #[must_use]
    pub fn layout_child_count(self) -> u32 {
        // SAFETY: self.0 is a live element pointer.
        unsafe { noesis_visual_children_count(self.0.as_ptr()) }
    }

    /// Borrow the `index`-th visual child for layout. `None` if out of range.
    #[must_use]
    pub fn layout_child(self, index: u32) -> Option<LayoutChild> {
        // SAFETY: self.0 is a live element pointer.
        let p = unsafe { noesis_visual_child(self.0.as_ptr(), index) };
        NonNull::new(p).map(|ptr| LayoutChild { ptr })
    }

    /// Set a read-only `Int32` DP via the privileged path (the analogue of a
    /// WPF `DependencyPropertyKey`). Ordinary [`Self::set_int32`] is a no-op on
    /// a read-only DP. Returns `false` on a bad instance / index.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_int32(self, prop_index: u32, value: i32) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            )
        }
    }
    /// Read-only setter for a `UInt32` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_uint32(self, prop_index: u32, value: u32) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const u32).cast(),
            )
        }
    }
    /// Read-only setter for a `Float` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_float(self, prop_index: u32, value: f32) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f32).cast(),
            )
        }
    }
    /// Read-only setter for a `Double` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_double(self, prop_index: u32, value: f64) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f64).cast(),
            )
        }
    }
    /// Read-only setter for a `Bool` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_bool(self, prop_index: u32, value: bool) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const bool).cast(),
            )
        }
    }
    /// Read-only setter for a `String` DP. See [`Self::set_readonly_int32`].
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_string(self, prop_index: u32, value: &str) -> bool {
        let cstr = CString::new(value).expect("string contained NUL");
        let ptr: *const c_char = cstr.as_ptr();
        // SAFETY: self.0 is a live instance pointer; cstr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&ptr as *const *const c_char).cast(),
            )
        }
    }
    /// Read-only setter for a `Point` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_point(self, prop_index: u32, x: f32, y: f32) -> bool {
        let arr = [x, y];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Read-only setter for a `Size` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_size(self, prop_index: u32, width: f32, height: f32) -> bool {
        let arr = [width, height];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Read-only setter for a `Vector` DP. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_vector(self, prop_index: u32, x: f32, y: f32) -> bool {
        let arr = [x, y];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Read-only setter for an enum DP (underlying `int32` member value). See
    /// [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_enum(self, prop_index: u32, value: i32) -> bool {
        // SAFETY: self.0 is a live instance pointer; value outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            )
        }
    }
}

unsafe extern "C" fn coerce_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    in_value: *const c_void,
    out_value: *mut c_void,
) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // Shared `&`: re-entrant per-class handler box (see `CoerceHandler`).
        let handler = &*userdata.cast::<Box<dyn CoerceHandler>>();
        let Some(inst) = NonNull::new(instance) else {
            return;
        };
        let value = decode_value(userdata, prop_index, in_value);
        let coerced = handler.coerce(Instance(inst), prop_index, value);
        encode_coerced(userdata, prop_index, coerced, out_value);
    })
}

/// Free trampoline for the donated coerce handler box. Mirrors
/// [`class_handler_free_trampoline`].
unsafe extern "C" fn coerce_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        forget_prop_types(userdata);
        drop(Box::from_raw(userdata.cast::<Box<dyn CoerceHandler>>()));
    })
}

unsafe fn encode_coerced(
    userdata: *mut c_void,
    prop_index: u32,
    coerced: Coerced,
    out_value: *mut c_void,
) {
    if out_value.is_null() {
        return;
    }
    // Validate the returned variant against the registered type; a mismatch is
    // ignored so the pre-coercion copy in `out_value` survives.
    let kind = lookup_prop_type(userdata, prop_index);
    match (kind, coerced) {
        (_, Coerced::Unchanged) => {}
        (Some(PropType::Int32), Coerced::Int32(v)) => *out_value.cast::<i32>() = v,
        (Some(PropType::UInt32), Coerced::UInt32(v)) => *out_value.cast::<u32>() = v,
        (Some(PropType::Float), Coerced::Float(v)) => *out_value.cast::<f32>() = v,
        (Some(PropType::Double), Coerced::Double(v)) => *out_value.cast::<f64>() = v,
        (Some(PropType::Bool), Coerced::Bool(v)) => *out_value.cast::<bool>() = v,
        (
            Some(PropType::Thickness),
            Coerced::Thickness {
                left,
                top,
                right,
                bottom,
            },
        ) => {
            let f = out_value.cast::<f32>();
            *f = left;
            *f.add(1) = top;
            *f.add(2) = right;
            *f.add(3) = bottom;
        }
        (Some(PropType::Color), Coerced::Color { r, g, b, a }) => {
            let f = out_value.cast::<f32>();
            *f = r;
            *f.add(1) = g;
            *f.add(2) = b;
            *f.add(3) = a;
        }
        (
            Some(PropType::Rect),
            Coerced::Rect {
                x,
                y,
                width,
                height,
            },
        ) => {
            let f = out_value.cast::<f32>();
            *f = x;
            *f.add(1) = y;
            *f.add(2) = width;
            *f.add(3) = height;
        }
        (Some(PropType::Point), Coerced::Point { x, y }) => {
            let f = out_value.cast::<f32>();
            *f = x;
            *f.add(1) = y;
        }
        (Some(PropType::Size), Coerced::Size { width, height }) => {
            let f = out_value.cast::<f32>();
            *f = width;
            *f.add(1) = height;
        }
        (Some(PropType::Vector), Coerced::Vector { x, y }) => {
            let f = out_value.cast::<f32>();
            *f = x;
            *f.add(1) = y;
        }
        // Variant / type mismatch: leave the passthrough copy in place.
        _ => {}
    }
}

unsafe extern "C" fn layout_measure_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    avail_w: f32,
    avail_h: f32,
    out_w: *mut f32,
    out_h: *mut f32,
) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // Shared `&`: re-entrant per-class handler box (see `LayoutHandler`).
        let handler = &*userdata.cast::<Box<dyn LayoutHandler>>();
        let size = match NonNull::new(instance) {
            Some(inst) => handler.measure(Instance(inst), Size::new(avail_w, avail_h)),
            None => Size::ZERO,
        };
        if !out_w.is_null() {
            *out_w = size.width;
        }
        if !out_h.is_null() {
            *out_h = size.height;
        }
    })
}

unsafe extern "C" fn layout_arrange_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    final_w: f32,
    final_h: f32,
    out_w: *mut f32,
    out_h: *mut f32,
) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // Shared `&`: re-entrant per-class handler box (see `LayoutHandler`).
        let handler = &*userdata.cast::<Box<dyn LayoutHandler>>();
        let size = match NonNull::new(instance) {
            Some(inst) => handler.arrange(Instance(inst), Size::new(final_w, final_h)),
            None => Size::new(final_w, final_h),
        };
        if !out_w.is_null() {
            *out_w = size.width;
        }
        if !out_h.is_null() {
            *out_h = size.height;
        }
    })
}

/// Free trampoline for the donated layout handler box. Mirrors
/// [`class_handler_free_trampoline`].
unsafe extern "C" fn layout_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn LayoutHandler>>()));
    })
}

/// Custom immediate-mode rendering, installed via [`ClassBuilder::set_render`].
/// The trampoline subclass's `OnRender` forwards here after the base
/// `OnRender` runs. Issue draw / push / pop commands through the borrowed
/// [`DrawingContext`]; it is valid only for the duration of the call.
///
/// `OnRender` fires during the renderer's render-tree update (drive it with a
/// [`View`](crate::view::View) + [`Renderer`](crate::view::Renderer) bound to a
/// [`RenderDevice`](crate::render_device::RenderDevice); see `tests/drawing.rs`).
/// Like the layout callbacks it runs on the view-driving thread; keep work small.
///
/// Takes `&self` (re-entrant: one handler box is shared by every instance of
/// the class, so rendering a nested element of the same type re-enters `render`
/// on the same box; use interior mutability for handler state).
pub trait RenderHandler: Send + 'static {
    /// Record this element's visual content into `ctx`. The element's render
    /// size is available via [`Instance::layout_child`] / the element's own
    /// `ActualWidth`/`ActualHeight` (read through a
    /// [`FrameworkElement`](crate::view::FrameworkElement)).
    fn render(&self, instance: Instance, ctx: DrawingContext<'_>);
}

unsafe extern "C" fn render_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    context: *mut c_void,
) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // Shared `&`: re-entrant per-class handler box (see `RenderHandler`).
        let handler = &*userdata.cast::<Box<dyn RenderHandler>>();
        let (Some(inst), Some(ctx)) = (NonNull::new(instance), NonNull::new(context)) else {
            return;
        };
        // SAFETY: `ctx` is the borrowed DrawingContext* delivered to OnRender, valid
        // only for this call; the `DrawingContext<'_>` lifetime keeps it scoped.
        let ctx = DrawingContext::from_raw(ctx);
        handler.render(Instance(inst), ctx);
    })
}

/// Free trampoline for the donated render handler box. Mirrors
/// [`class_handler_free_trampoline`].
unsafe extern "C" fn render_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn RenderHandler>>()));
    })
}
