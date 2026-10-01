//! Custom XAML classes implemented in Rust.
//!
//! Register a new type that XAML can instantiate by name (`<game:Badge/>`),
//! with dependency properties (DPs) whose changes call back into Rust. The
//! class derives from one of the [`ClassBase`] Noesis types and can also take
//! over coercion ([`CoerceHandler`]), layout ([`LayoutHandler`]) and rendering
//! ([`RenderHandler`]).
//!
//! # Lifecycle
//!
//! 1. Call [`init`](crate::init).
//! 2. Create a [`ClassBuilder`], add properties with
//!    [`add_property`](ClassBuilder::add_property) and friends, and call
//!    [`register`](ClassBuilder::register) to get a [`ClassRegistration`].
//! 3. Load XAML that uses the class, or create instances directly with
//!    [`ClassRegistration::create_instance`]. Every property change, from
//!    XAML, a binding or Rust, calls [`PropertyChangeHandler::on_changed`].
//! 4. Read and write properties through an [`Instance`], using the index
//!    returned when the property was added.
//! 5. Drop the [`ClassRegistration`] to stop new instances being created.
//!    Existing instances keep working; the handlers are freed when the last
//!    one goes away.
//!
//! ```no_run
//! use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
//! use noesis_runtime::ffi::{ClassBase, PropType};
//!
//! struct Badge;
//!
//! impl PropertyChangeHandler for Badge {
//!     fn on_changed(&self, _instance: Instance, _prop_index: u32, value: PropertyValue<'_>) {
//!         if let PropertyValue::Int32(count) = value {
//!             println!("count is now {count}");
//!         }
//!     }
//! }
//!
//! noesis_runtime::init();
//! let mut builder = ClassBuilder::new("Game.Badge", ClassBase::ContentControl, Badge);
//! let count = builder.add_property("Count", PropType::Int32);
//! let registration = builder.register().expect("name already registered");
//!
//! let badge = registration.create_instance().expect("instance");
//! badge.handle().set_int32(count, 5); // prints "count is now 5"
//! ```
//!
//! # Threading and re-entrancy
//!
//! Callbacks run on the thread that drives the view. Keep handlers short and
//! send work elsewhere through a channel if needed.
//!
//! Writing a property from Rust calls the change handler synchronously before
//! the setter returns, including from inside a handler. All handler methods
//! take `&self` for that reason; keep mutable state in a `Cell`, `RefCell`,
//! `Mutex` or atomic.

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

/// Called exactly once by C++ when the class data is freed: at unregister if
/// no instances exist, otherwise when the last instance dies.
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

/// The `(width, height)` of an image source, or `None` if `image_source` is
/// not an `ImageSource`. Handy in a [`PropertyChangeHandler`] that derives
/// other properties from an image's size; [`Instance::get_image_source_size`]
/// is the safe form for a property you registered.
///
/// # Safety
///
/// `image_source` must point to a live Noesis `BaseComponent`, such as the
/// pointer in a [`PropertyValue::ImageSource`] during its callback. No
/// reference is taken or released.
#[must_use]
pub unsafe fn image_source_size(image_source: NonNull<c_void>) -> Option<(f32, f32)> {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    let ok = noesis_image_source_get_size(image_source.as_ptr(), &mut w, &mut h);
    ok.then_some((w, h))
}

/// Receives property changes for every instance of a class.
///
/// One handler serves all instances of the class. It takes `&self` because it
/// is re-entrant: a handler that writes another property of the instance (a
/// computed property) is called again for that write before the outer call
/// returns. Keep mutable state in a `Cell`, `RefCell`, `Mutex` or atomic.
pub trait PropertyChangeHandler: Send + 'static {
    /// Called after property `prop_index` of `instance` changed to `value`.
    /// `prop_index` is the index returned when the property was added.
    fn on_changed(&self, instance: Instance, prop_index: u32, value: PropertyValue<'_>);
}

/// A property value passed to [`PropertyChangeHandler`] and [`CoerceHandler`].
/// The variant matches the property's registered [`PropType`]. Colors are
/// floats in `0..=1`; lengths are device-independent pixels.
///
/// `String`, `ImageSource` and `BaseComponent` borrow Noesis-owned data that is
/// valid only during the callback. Copy the string out if you need it later.
/// `String` is `None` for a null or non-UTF-8 value.
#[derive(Debug)]
pub enum PropertyValue<'a> {
    Int32(i32),
    UInt32(u32),
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
    Point {
        x: f32,
        y: f32,
    },
    Size {
        width: f32,
        height: f32,
    },
    /// A `Noesis::Vector2`.
    Vector {
        x: f32,
        y: f32,
    },
    /// The integer value of a runtime enum member (see
    /// [`ClassBuilder::add_enum_property`]).
    Enum(i32),
    /// Borrowed `Noesis::ImageSource*`, or `None` when unset.
    ImageSource(Option<NonNull<c_void>>),
    /// Borrowed `Noesis::BaseComponent*`, or `None` when unset.
    BaseComponent(Option<NonNull<c_void>>),
}

struct PropSpec {
    name: CString,
    kind: PropType,
    default: OwnedDefault,
    options: PropertyOptions,
    enum_type: Option<CString>,
}

/// Defines one custom class. Add properties and optional handlers, then call
/// [`register`](Self::register).
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

/// A string default for the FFI, which takes `const char* const*`. `ptr`
/// points into `_bytes`' heap buffer, so it stays valid when this moves.
struct StringDefault {
    _bytes: CString,
    ptr: *const c_char,
}

impl<H: PropertyChangeHandler> ClassBuilder<H> {
    /// Starts a class named `name`, derived from `base`, whose property
    /// changes go to `handler`.
    ///
    /// `name` is the full type name XAML resolves, such as `"Game.Badge"`,
    /// which XAML reaches with `xmlns:game="clr-namespace:Game"` and
    /// `<game:Badge/>`.
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

    /// Adds a property with its type's zero default and returns its index.
    /// Indices count up from 0 in the order properties are added, across all
    /// `add_*` methods. Use the index with [`Instance`] and to recognise the
    /// property in your handlers.
    ///
    /// For an enum-typed property use [`add_enum_property`](Self::add_enum_property)
    /// instead of [`PropType::Enum`].
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn add_property(&mut self, name: &str, kind: PropType) -> u32 {
        self.add_property_with(name, kind, PropertyDefault::None)
    }

    /// Like [`add_property`](Self::add_property), with a starting value.
    /// `default` must match `kind`. `ImageSource` and `BaseComponent`
    /// properties have no default variant and start null.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn add_property_with(
        &mut self,
        name: &str,
        kind: PropType,
        default: PropertyDefault<'_>,
    ) -> u32 {
        self.add_property_ex(name, kind, default, PropertyOptions::default())
    }

    /// Like [`add_property_with`](Self::add_property_with), with
    /// [`PropertyOptions`]: layout and render invalidation flags
    /// ([`fpm_options`]), read-only access, and coercion.
    ///
    /// Coercion needs a handler from [`set_coerce`](Self::set_coerce). It is
    /// ignored for `UInt64`, `String`, `ImageSource` and `BaseComponent`
    /// properties. Only the first 32 properties of a class can be coerced: a
    /// coerced property at index 32 or above makes
    /// [`register`](Self::register) fail.
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

    /// Adds a property typed as the runtime enum `enum_type_name` (see
    /// [`crate::reflection::register_enum`]) and returns its index. XAML and
    /// styles can then set it by member name. Values are the members' `i32`s;
    /// `default` is the starting value. Enum properties cannot be coerced, and
    /// `options.coerce` is ignored.
    ///
    /// The enum must be registered before [`register`](Self::register) runs,
    /// or `register` returns `None`.
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

    /// Sets the class's [`CoerceHandler`]. It applies only to properties added
    /// with [`PropertyOptions::coerce`] set. Calling this again replaces the
    /// previous handler.
    pub fn set_coerce(&mut self, handler: impl CoerceHandler) {
        self.coerce = Some(Box::new(handler));
    }

    /// Sets a [`LayoutHandler`] that replaces the class's measure and arrange
    /// passes. Without one the base class lays out as usual. Has no effect on
    /// a [`ClassBase::Freezable`] class.
    pub fn set_layout(&mut self, handler: impl LayoutHandler) {
        self.layout = Some(Box::new(handler));
    }

    /// Sets a [`RenderHandler`] that draws extra content on top of what the
    /// base class renders. Has no effect on a [`ClassBase::Freezable`] class.
    pub fn set_render(&mut self, handler: impl RenderHandler) {
        self.render = Some(Box::new(handler));
    }

    /// Registers the class with Noesis.
    ///
    /// Returns `None` if the name is already a registered type or a property
    /// fails to register (for example an unknown enum type, or a coerced
    /// property past index 31). A class name stays taken until
    /// [`shutdown`](crate::shutdown), even after its [`ClassRegistration`] is
    /// dropped, so registering the same name twice fails.
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

        // outer Box: thin pointer for the C ABI userdata
        let boxed: Box<Box<dyn PropertyChangeHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // before the FFI call: C++ may fire a callback during registration
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
            // C side returns NULL before taking ownership of the box
            forget_prop_types(userdata.cast());
            unsafe { drop(Box::from_raw(userdata)) };
            return None;
        };

        for spec in &props {
            let idx = if let Some(enum_type) = &spec.enum_type {
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
                // unregister frees the donated handler box (no instances yet);
                // coerce / layout / render were never donated
                unsafe { noesis_class_unregister(token.as_ptr()) };
                drop(coerce);
                drop(layout);
                drop(render);
                return None;
            }
        }

        // coerce / layout / render boxes cross the C ABI; C++ frees each via
        // its free trampoline when the ClassData dies
        if let Some(handler) = coerce {
            let boxed: Box<Box<dyn CoerceHandler>> = Box::new(handler);
            let coerce_ud = Box::into_raw(boxed);
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

/// Starting value for [`ClassBuilder::add_property_with`]. The variant must
/// match the property's [`PropType`]; [`None`](Self::None) uses the type's
/// zero value.
#[derive(Debug, Clone, Copy)]
pub enum PropertyDefault<'a> {
    None,
    Int32(i32),
    UInt32(u32),
    UInt64(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    /// A string containing NUL falls back to `""`.
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
    /// A runtime enum member's value, for example from
    /// [`EnumType::value_from_name`](crate::reflection::EnumType::value_from_name).
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
    /// Null means "use the type's default".
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
                // `const char* const*`, read only during the registration call
                Some(slot) => (&slot.ptr as *const *const c_char).cast(),
                // interior NUL: C++ uses ""
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

/// A registered class. Dropping it stops new instances from being created;
/// instances that already exist keep working, and the handlers are freed when
/// the last of them is destroyed. The class name stays reserved until
/// [`shutdown`](crate::shutdown).
#[must_use = "dropping the guard immediately clears the registration"]
pub struct ClassRegistration {
    token: NonNull<c_void>,
    _name: CString,
    num_props: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClassRegistration {}

impl ClassRegistration {
    /// Number of properties on the class.
    #[must_use]
    pub fn num_properties(&self) -> u32 {
        self.num_props
    }

    /// Opaque pointer identifying the class on the C side, valid while `self`
    /// is alive. Only useful for passing to other FFI code.
    pub fn token(&self) -> NonNull<c_void> {
        self.token
    }

    /// Creates an instance from Rust, without XAML. Returns `None` only if
    /// Noesis returns null.
    ///
    /// An instance works as a view model: pass it to
    /// [`FrameworkElement::set_data_context`](crate::view::FrameworkElement::set_data_context),
    /// bind to its properties with `{Binding Count}` in XAML, and write them
    /// through [`ClassInstance::handle`]. Bound elements pick up the change on
    /// the next view update.
    #[must_use]
    pub fn create_instance(&self) -> Option<ClassInstance> {
        // SAFETY: `self.token` is a live ClassData* for the lifetime of `self`.
        let ptr = unsafe { noesis_class_create_instance(self.token.as_ptr()) };
        NonNull::new(ptr).map(|ptr| ClassInstance { ptr })
    }
}

/// An instance created by [`ClassRegistration::create_instance`]. Owns one
/// Noesis reference, released on drop; it may outlive the registration.
pub struct ClassInstance {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClassInstance {}

impl ClassInstance {
    /// An [`Instance`] for reading and writing properties. It does not keep
    /// the object alive.
    #[must_use]
    pub fn handle(&self) -> Instance {
        // SAFETY: self.ptr is a live instance pointer for the lifetime of self.
        unsafe { Instance::from_raw(self.ptr) }
    }

    /// Raw `Noesis::BaseComponent*`, borrowed for the lifetime of `self`. Pass
    /// it to APIs that take one, such as [`Instance::set_component`].
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Makes a [`ClassBase::Freezable`] instance immutable. Returns `false` if
    /// the class is not `Freezable` or [`can_freeze`](Self::can_freeze) is
    /// `false`.
    pub fn freeze(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_freezable_freeze(self.ptr.as_ptr()) }
    }

    /// Whether the instance is frozen. Always `false` for non-`Freezable`
    /// classes.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_freezable_is_frozen(self.ptr.as_ptr()) }
    }

    /// Whether [`freeze`](Self::freeze) would succeed. Always `false` for
    /// non-`Freezable` classes.
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
        // Instances may outlive this; C++ frees the handler boxes when the
        // last one dies.
        // SAFETY: `self.token` came from `ClassBuilder::register`; unregistered
        // exactly once here.
        unsafe { noesis_class_unregister(self.token.as_ptr()) };
    }
}

/// A non-owning handle to an instance of a custom class, for reading and
/// writing its properties. Handlers receive one; [`ClassInstance::handle`]
/// gives one for instances you created.
///
/// It holds no reference, so use it only while the object is alive: inside a
/// handler call, or while you hold the [`ClassInstance`].
///
/// # Property access
///
/// `prop_index` is the index returned when the property was added. Each
/// `set_*` / `get_*` must match the property's registered [`PropType`]: the
/// type is not checked, and a mismatch reads or writes the wrong number of
/// bytes. An out-of-range index is ignored (`None` from getters).
///
/// Setters do nothing on a read-only property; use the `set_readonly_*`
/// methods. A setter that changes the value calls the class's
/// [`PropertyChangeHandler`] before returning.
#[derive(Copy, Clone, Debug)]
pub struct Instance(NonNull<c_void>);

impl Instance {
    /// Wraps a raw instance pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must come from [`Instance::as_ptr`] or [`ClassInstance::raw`].
    pub unsafe fn from_raw(ptr: NonNull<c_void>) -> Self {
        Self(ptr)
    }

    /// The raw instance pointer (a `Noesis::BaseComponent*`).
    pub fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Sets an `Int32` property.
    pub fn set_int32(self, prop_index: u32, value: i32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            );
        }
    }
    /// Sets a `UInt64` property. Useful for tagging a row view model with an
    /// ID (such as an ECS entity) that an event handler can read back from the
    /// sender's `DataContext`.
    pub fn set_u64(self, prop_index: u32, value: u64) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const u64).cast(),
            );
        }
    }
    /// Sets a `Float` property.
    pub fn set_float(self, prop_index: u32, value: f32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f32).cast(),
            );
        }
    }
    /// Sets a `Double` property.
    pub fn set_double(self, prop_index: u32, value: f64) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const f64).cast(),
            );
        }
    }
    /// Sets a `Bool` property.
    pub fn set_bool(self, prop_index: u32, value: bool) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const bool).cast(),
            );
        }
    }
    /// Sets a `String` property.
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
    /// Sets a `Thickness` property from its four edge widths, in
    /// device-independent pixels.
    pub fn set_thickness(self, prop_index: u32, left: f32, top: f32, right: f32, bottom: f32) {
        let arr = [left, top, right, bottom];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets a `Color` property from components in `0..=1`.
    pub fn set_color(self, prop_index: u32, r: f32, g: f32, b: f32, a: f32) {
        let arr = [r, g, b, a];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets a `Rect` property.
    pub fn set_rect(self, prop_index: u32, x: f32, y: f32, width: f32, height: f32) {
        let arr = [x, y, width, height];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets a `Point` property.
    pub fn set_point(self, prop_index: u32, x: f32, y: f32) {
        let arr = [x, y];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets a `Size` property.
    pub fn set_size(self, prop_index: u32, width: f32, height: f32) {
        let arr = [width, height];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets a `Vector` property.
    pub fn set_vector(self, prop_index: u32, x: f32, y: f32) {
        let arr = [x, y];
        unsafe {
            noesis_instance_set_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast());
        }
    }
    /// Sets an enum property (from
    /// [`ClassBuilder::add_enum_property`]) to a member's integer value.
    pub fn set_enum(self, prop_index: u32, value: i32) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&value as *const i32).cast(),
            );
        }
    }
    /// Sets an `ImageSource` or `BaseComponent` property to `component`, or
    /// clears it with null. The property takes its own reference; yours is
    /// not consumed. For commands, [`set_command`](Self::set_command) is the
    /// safe form.
    ///
    /// # Safety
    ///
    /// `component` must be null or a live `Noesis::BaseComponent*`, such as
    /// [`ClassInstance::raw`]. For an `ImageSource` property it must be an
    /// `ImageSource`.
    pub unsafe fn set_component(self, prop_index: u32, component: *mut c_void) {
        unsafe {
            noesis_instance_set_property(
                self.0.as_ptr(),
                prop_index,
                (&component as *const *mut c_void).cast(),
            );
        }
    }

    /// Sets a [`PropType::BaseComponent`] property to `command`, which can be
    /// any [`AsCommand`](crate::commands::AsCommand) type. The property takes
    /// its own reference. With the instance as a `DataContext`, XAML binds to
    /// it with `Command="{Binding PropertyName}"`. See [`crate::commands`].
    pub fn set_command(self, prop_index: u32, command: &impl crate::commands::AsCommand) {
        // SAFETY: `command.command_ptr()` is a live ICommand* (a BaseComponent*
        // at runtime) borrowed for the duration of this synchronous call; the
        // DP stores its own reference, leaving the caller's ownership intact.
        unsafe { self.set_component(prop_index, command.command_ptr()) }
    }

    /// Reads an `Int32` property.
    pub fn get_int32(self, prop_index: u32) -> Option<i32> {
        let mut out: i32 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut i32).cast())
        };
        ok.then_some(out)
    }
    /// Reads a `UInt64` property.
    pub fn get_u64(self, prop_index: u32) -> Option<u64> {
        let mut out: u64 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut u64).cast())
        };
        ok.then_some(out)
    }
    /// Reads a `Float` property.
    pub fn get_float(self, prop_index: u32) -> Option<f32> {
        let mut out: f32 = 0.0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut f32).cast())
        };
        ok.then_some(out)
    }
    /// Reads a `String` property. Invalid UTF-8 is replaced with `U+FFFD`.
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
        // SAFETY: p is a NUL-terminated string in Noesis-owned storage, copied
        // out before returning.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
    /// Reads a `Thickness` property as `(left, top, right, bottom)`.
    pub fn get_thickness(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    /// Reads a `Rect` property as `(x, y, width, height)`.
    pub fn get_rect(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }
    /// Reads a `Point` property as `(x, y)`.
    pub fn get_point(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Reads a `Size` property as `(width, height)`.
    pub fn get_size(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Reads a `Vector` property as `(x, y)`.
    pub fn get_vector(self, prop_index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1]))
    }
    /// Reads an enum property as its member's integer value.
    pub fn get_enum(self, prop_index: u32) -> Option<i32> {
        let mut out: i32 = 0;
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, (&mut out as *mut i32).cast())
        };
        ok.then_some(out)
    }
    /// Reads a `Color` property as `(r, g, b, a)`, each in `0..=1`.
    pub fn get_color(self, prop_index: u32) -> Option<(f32, f32, f32, f32)> {
        let mut out = [0.0f32; 4];
        let ok = unsafe {
            noesis_instance_get_property(self.0.as_ptr(), prop_index, out.as_mut_ptr().cast())
        };
        ok.then_some((out[0], out[1], out[2], out[3]))
    }

    /// The `(width, height)` of the image in an `ImageSource` or
    /// `BaseComponent` property. `None` if the property is unset or does not
    /// hold an `ImageSource`.
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

unsafe extern "C" fn prop_changed_trampoline(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    value_ptr: *const c_void,
) {
    crate::panic_guard::guard(|| {
        // shared `&`, never `&mut`: re-entered by a `set_*` inside `on_changed`
        let handler = &*userdata.cast::<Box<dyn PropertyChangeHandler>>();
        let Some(instance) = NonNull::new(instance) else {
            return;
        };

        // the callback carries no type tag; CLASS_PROP_TYPES supplies it
        let value = decode_value(userdata, prop_index, value_ptr);
        handler.on_changed(Instance(instance), prop_index, value);
    })
}

// Property types per class, keyed by handler userdata pointer (unique per
// registration), so trampolines can decode `value_ptr`.
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

/// Flags for [`PropertyOptions::fpm_options`] that make a property change
/// invalidate layout or rendering, or inherit down the tree. OR them together.
/// Values match Noesis's `FrameworkPropertyMetadataOptions`.
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

/// Options for [`ClassBuilder::add_property_ex`]. The default is a plain
/// writable property.
#[derive(Clone, Copy, Debug, Default)]
pub struct PropertyOptions {
    /// [`fpm_options`] flags, combined with `|`.
    pub fpm_options: u32,
    /// Reject writes from XAML, bindings and the [`Instance`] `set_*` methods.
    /// Only the `set_readonly_*` methods, such as
    /// [`Instance::set_readonly_int32`], can change it.
    pub read_only: bool,
    /// Pass values through the class's [`CoerceHandler`]. See
    /// [`ClassBuilder::add_property_ex`] for which properties support it.
    pub coerce: bool,
}

/// Result of [`CoerceHandler::coerce`]. The variant must match the
/// property's [`PropType`]; a mismatched variant keeps the input value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coerced {
    /// Keep the input value.
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

/// Adjusts values of coerced properties before they take effect, for example
/// to clamp a number to a range. Set with [`ClassBuilder::set_coerce`]; a
/// property opts in through [`PropertyOptions::coerce`].
///
/// Noesis calls it whenever a coerced property's effective value is computed.
/// Getters then return the coerced value, while the value that was set is kept
/// and coerced again on the next recompute.
///
/// Takes `&self` because it is re-entrant: reading or writing another coerced
/// property from inside `coerce` calls it again. Keep mutable state in a
/// `Cell`, `RefCell`, `Mutex` or atomic.
pub trait CoerceHandler: Send + 'static {
    /// Returns the value property `prop_index` should take given the incoming
    /// `value`, or [`Coerced::Unchanged`].
    fn coerce(&self, instance: Instance, prop_index: u32, value: PropertyValue<'_>) -> Coerced;
}

/// A width and height in device-independent pixels, used by
/// [`LayoutHandler`]. Available sizes may be `f32::INFINITY`.
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

/// Custom measure and arrange for a class, set with
/// [`ClassBuilder::set_layout`]. It replaces the base class's
/// `MeasureOverride` and `ArrangeOverride`.
///
/// Return a fixed size from [`measure`](Self::measure) for a self-sized
/// element. To lay out children, walk them with
/// [`Instance::layout_child_count`] and [`Instance::layout_child`], and call
/// [`LayoutChild::measure`] and [`LayoutChild::arrange`] on each.
///
/// The default `measure` asks for zero space and the default `arrange` accepts
/// the final size, so you can override just one.
///
/// Methods take `&self` because they are re-entrant: one handler serves every
/// instance, and a panel containing children of its own class lays them out
/// from inside its own call. Keep mutable state in a `Cell`, `RefCell`,
/// `Mutex` or atomic.
pub trait LayoutHandler: Send + 'static {
    /// Returns the size the element wants, given `available` space. Measure
    /// any children here.
    fn measure(&self, instance: Instance, available: Size) -> Size {
        let _ = (instance, available);
        Size::ZERO
    }

    /// Positions children within `final_size` and returns the size used.
    fn arrange(&self, instance: Instance, final_size: Size) -> Size {
        let _ = instance;
        final_size
    }
}

/// A child element, from [`Instance::layout_child`]. Use it only inside the
/// [`LayoutHandler`] call that produced it; it holds no reference.
pub struct LayoutChild {
    ptr: NonNull<c_void>,
}

impl LayoutChild {
    /// Measures the child against `available`. Returns `false` if the child is
    /// not a `UIElement`.
    pub fn measure(&self, available: Size) -> bool {
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        unsafe { noesis_uielement_measure(self.ptr.as_ptr(), available.width, available.height) }
    }

    /// Places the child at `(x, y)` with size `(w, h)`, in the parent's
    /// coordinates. Returns `false` if the child is not a `UIElement`.
    pub fn arrange(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        unsafe { noesis_uielement_arrange(self.ptr.as_ptr(), x, y, w, h) }
    }

    /// The child's desired size from its last [`measure`](Self::measure), or
    /// `None` if it is not a `UIElement`.
    #[must_use]
    pub fn desired_size(&self) -> Option<Size> {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: ptr is a live UIElement* borrowed for the callback.
        let ok = unsafe { noesis_uielement_desired_size(self.ptr.as_ptr(), &mut w, &mut h) };
        ok.then_some(Size::new(w, h))
    }

    /// The borrowed `Noesis::UIElement*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Instance {
    /// Number of visual children. Call only from a [`LayoutHandler`].
    #[must_use]
    pub fn layout_child_count(self) -> u32 {
        // SAFETY: self.0 is a live element pointer.
        unsafe { noesis_visual_children_count(self.0.as_ptr()) }
    }

    /// The visual child at `index`, or `None` if out of range. Call only from a
    /// [`LayoutHandler`].
    #[must_use]
    pub fn layout_child(self, index: u32) -> Option<LayoutChild> {
        // SAFETY: self.0 is a live element pointer.
        let p = unsafe { noesis_visual_child(self.0.as_ptr(), index) };
        NonNull::new(p).map(|ptr| LayoutChild { ptr })
    }

    /// Sets an `Int32` property registered with [`PropertyOptions::read_only`],
    /// which the ordinary setters cannot change. Works on writable properties
    /// too. Returns `false` for a destroyed instance or an out-of-range index.
    /// The type rules in [Property access](Self#property-access) apply.
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
    /// Sets a read-only `UInt32` property. See [`Self::set_readonly_int32`].
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
    /// Sets a read-only `Float` property. See [`Self::set_readonly_int32`].
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
    /// Sets a read-only `Double` property. See [`Self::set_readonly_int32`].
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
    /// Sets a read-only `Bool` property. See [`Self::set_readonly_int32`].
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
    /// Sets a read-only `String` property. See [`Self::set_readonly_int32`].
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
    /// Sets a read-only `Point` property. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_point(self, prop_index: u32, x: f32, y: f32) -> bool {
        let arr = [x, y];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Sets a read-only `Size` property. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_size(self, prop_index: u32, width: f32, height: f32) -> bool {
        let arr = [width, height];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Sets a read-only `Vector` property. See [`Self::set_readonly_int32`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_readonly_vector(self, prop_index: u32, x: f32, y: f32) -> bool {
        let arr = [x, y];
        // SAFETY: self.0 is a live instance pointer; arr outlives the call.
        unsafe {
            noesis_instance_set_readonly_property(self.0.as_ptr(), prop_index, arr.as_ptr().cast())
        }
    }
    /// Sets a read-only enum property to a member's integer value. See
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
        let handler = &*userdata.cast::<Box<dyn CoerceHandler>>();
        let Some(inst) = NonNull::new(instance) else {
            return;
        };
        let value = decode_value(userdata, prop_index, in_value);
        let coerced = handler.coerce(Instance(inst), prop_index, value);
        encode_coerced(userdata, prop_index, coerced, out_value);
    })
}

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
    // C++ pre-fills `out_value` with the input, so a mismatch passes it through
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

unsafe extern "C" fn layout_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn LayoutHandler>>()));
    })
}

/// Immediate-mode drawing for a class, set with [`ClassBuilder::set_render`].
/// It runs from the element's `OnRender`, after the base class has drawn.
///
/// Noesis calls it when the element's render content is rebuilt, which needs a
/// [`View`](crate::view::View) with a [`Renderer`](crate::view::Renderer) on a
/// [`RenderDevice`](crate::render_device::RenderDevice). The drawing is
/// retained and redrawn only when the element's render is invalidated, for
/// example by a property flagged [`fpm_options::AFFECTS_RENDER`].
///
/// Takes `&self` because it is re-entrant: one handler serves every instance,
/// including nested instances of the same class. Keep mutable state in a
/// `Cell`, `RefCell`, `Mutex` or atomic.
pub trait RenderHandler: Send + 'static {
    /// Draws the element's content into `ctx`, in the element's local
    /// coordinates. `ctx` is valid only for this call. No size is passed; a
    /// class that needs it can record the arranged size from its
    /// [`LayoutHandler`].
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

unsafe extern "C" fn render_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn RenderHandler>>()));
    })
}
