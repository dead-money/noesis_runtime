//! Commands: `ICommand` implementations backed by Rust, routed commands, and
//! command bindings.
//!
//! # Rust-backed commands
//!
//! A [`Command`] is an `ICommand` whose `CanExecute` / `Execute` call a Rust
//! [`CommandHandler`]. To bind it from XAML:
//!
//! 1. Register a view-model class with a `BaseComponent` dependency property
//!    (see [`ClassBuilder`](crate::classes::ClassBuilder)).
//! 2. Set that property to the command with
//!    [`Instance::set_command`](crate::classes::Instance::set_command).
//! 3. Make the instance the `DataContext`
//!    ([`FrameworkElement::set_data_context`](crate::view::FrameworkElement::set_data_context)).
//! 4. Write `<Button Command="{Binding ThatProperty}"/>` in XAML.
//!
//! Clicking the button runs [`CommandHandler::execute`]. The button also calls
//! [`CommandHandler::can_execute`] to set its `IsEnabled`; call
//! [`Command::raise_can_execute_changed`] when that answer changes so bound
//! controls ask again.
//!
//! # Routed commands
//!
//! A [`RoutedCommand`] or [`RoutedUICommand`] carries no logic. Executing it on
//! an element routes up the tree to the first [`CommandBinding`] for that
//! command. The framework's built-in commands ([`ApplicationCommand`],
//! [`ComponentCommand`]) are routed commands too, reached as a
//! [`BorrowedCommand`].
//!
//! # Lifetime
//!
//! [`Command`] holds one reference, released on drop. A binding that uses the
//! command holds its own, so the command and its handler stay alive, and keep
//! working, until the last reference goes. The handler is freed exactly once,
//! by the C++ destructor.
//!
//! # Threading
//!
//! Handlers run inside Noesis's input and binding processing, on the thread
//! that drives the view. Keep them short; queue heavy work elsewhere.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    CommandVTable, noesis_application_command, noesis_base_component_release,
    noesis_command_binding_attach, noesis_command_binding_create, noesis_command_binding_destroy,
    noesis_command_create, noesis_command_destroy, noesis_command_raise_can_execute_changed,
    noesis_component_command, noesis_routed_command_can_execute, noesis_routed_command_create,
    noesis_routed_command_execute, noesis_routed_command_get_name, noesis_routed_ui_command_create,
    noesis_routed_ui_command_get_text, noesis_routed_ui_command_set_text, noesis_unbox_bool,
    noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
};
use crate::view::FrameworkElement;

/// A borrowed C string (`*const c_char`) → owned `String`, or `None` if null.
unsafe fn cstr_opt(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

/// A command's `CommandParameter`: a borrowed, boxed `Noesis::BaseComponent*`,
/// or nothing. The typed accessors return `None` when the boxed type doesn't
/// match. Inside a handler the pointer is valid only for the callback.
pub struct CommandParameterValue(Option<NonNull<c_void>>);

impl CommandParameterValue {
    /// Wrap a raw `BaseComponent*` to pass when executing a command yourself
    /// (e.g. [`RoutedCommand::execute`]). Null means no parameter.
    #[must_use]
    pub fn new(raw: *mut c_void) -> Self {
        Self(NonNull::new(raw))
    }

    /// Whether there is no parameter (a null pointer).
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// Raw borrowed `Noesis::BaseComponent*` (the boxed value), or `None` when
    /// no parameter was supplied.
    #[must_use]
    pub fn raw(&self) -> Option<NonNull<c_void>> {
        self.0
    }

    /// Unbox a `bool`. `None` on a type mismatch or no parameter.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let p = self.0?;
        let mut out = false;
        // SAFETY: p is a live boxed BaseComponent* for the callback; out is valid.
        let ok = unsafe { noesis_unbox_bool(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `i32`. `None` on a type mismatch or no parameter.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let p = self.0?;
        let mut out = 0i32;
        // SAFETY: as in `as_bool`.
        let ok = unsafe { noesis_unbox_int32(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `f64`. `None` on a type mismatch or no parameter.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let p = self.0?;
        let mut out = 0.0f64;
        // SAFETY: as in `as_bool`.
        let ok = unsafe { noesis_unbox_double(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrow a boxed string. `None` on a type mismatch, no parameter, or
    /// invalid UTF-8. A literal `CommandParameter="..."` in XAML arrives as a
    /// string, so this is the usual accessor for constant parameters.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let p = self.0?;
        // SAFETY: p is a live boxed BaseComponent* for the callback.
        let s = unsafe { noesis_unbox_string(p.as_ptr()) };
        if s.is_null() {
            return None;
        }
        // SAFETY: s is a borrowed NUL-terminated string valid for the callback.
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}

/// The logic behind a [`Command`].
///
/// The handler is `Send` because [`Command`] is, and `'static` because the
/// command can outlive its [`Command`] handle while a binding holds it.
pub trait CommandHandler: Send + 'static {
    /// Whether the command can run now. Defaults to `true`. Bound controls use
    /// it to set `IsEnabled`. Call [`Command::raise_can_execute_changed`] when
    /// the answer changes.
    fn can_execute(&self, _param: CommandParameterValue) -> bool {
        true
    }

    /// Run the command, e.g. on a bound `Button` click. Controls check
    /// [`Self::can_execute`] first; the command itself does not.
    ///
    /// Takes `&self` because calls can re-enter the same handler (`execute` may
    /// trigger a `can_execute` query or activate another control bound to the
    /// same command). Use interior mutability for state.
    fn execute(&self, param: CommandParameterValue);
}

/// Any `Fn(CommandParameterValue)` closure is a handler whose `can_execute` is
/// always `true`. Implement [`CommandHandler`] on a type when you need to
/// control `can_execute`.
impl<F: Fn(CommandParameterValue) + Send + 'static> CommandHandler for F {
    fn execute(&self, param: CommandParameterValue) {
        self(param);
    }
}

/// Shared by every command; the trampolines recover the handler from `userdata`.
static COMMAND_VTABLE: CommandVTable = CommandVTable {
    can_execute: command_can_execute_trampoline,
    execute: command_execute_trampoline,
};

/// SAFETY: `userdata` is the `Box<Box<dyn CommandHandler>>` leaked in
/// [`Command::new`], alive until the free trampoline runs.
unsafe extern "C" fn command_can_execute_trampoline(
    userdata: *mut c_void,
    param: *mut c_void,
) -> bool {
    crate::panic_guard::guard(|| {
        let handler = &*userdata.cast::<Box<dyn CommandHandler>>();
        handler.can_execute(CommandParameterValue::new(param))
    })
}

/// SAFETY: see [`command_can_execute_trampoline`].
unsafe extern "C" fn command_execute_trampoline(userdata: *mut c_void, param: *mut c_void) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `CommandHandler::execute`).
        let handler = &*userdata.cast::<Box<dyn CommandHandler>>();
        handler.execute(CommandParameterValue::new(param));
    })
}

/// SAFETY: `userdata` was produced by [`Command::new`] and C++ owns it; this
/// is the matching `Box::from_raw` that ends that ownership, run exactly once.
unsafe extern "C" fn command_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn CommandHandler>>()));
    })
}

/// An `ICommand` backed by a Rust [`CommandHandler`]. Bind it to a view-model
/// property with [`Instance::set_command`](crate::classes::Instance::set_command);
/// see the [module docs](self). Holds one reference, released on drop.
pub struct Command {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Command {}

impl Command {
    /// Create a command from a [`CommandHandler`] or a
    /// `Fn(CommandParameterValue)` closure.
    ///
    /// # Panics
    ///
    /// Never in practice: the C side returns null only for a null vtable.
    #[must_use]
    pub fn new<H: CommandHandler>(handler: H) -> Self {
        // Double box: `Box<dyn _>` is a fat pointer; the C ABI needs a thin one.
        let boxed: Box<Box<dyn CommandHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // SAFETY: vtable is a 'static valid pointer; userdata is freshly
        // leaked and ownership transfers to C++; free trampoline is extern "C".
        let ptr = unsafe {
            noesis_command_create(&COMMAND_VTABLE, userdata.cast(), command_free_trampoline)
        };

        match NonNull::new(ptr) {
            Some(ptr) => Command { ptr },
            None => {
                // SAFETY: userdata came from Box::into_raw above; C++ never
                // stored it (null return = nothing took ownership).
                unsafe { drop(Box::from_raw(userdata)) };
                unreachable!("noesis_command_create returned null for a non-null vtable");
            }
        }
    }

    /// Raw `Noesis::BaseComponent*` (an `ICommand`) for APIs that take a
    /// borrowed component, such as
    /// [`Instance::set_component`](crate::classes::Instance::set_component).
    /// Valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Raise `CanExecuteChanged` so bound controls call
    /// [`CommandHandler::can_execute`] again and update `IsEnabled`.
    pub fn raise_can_execute_changed(&self) {
        // SAFETY: self.ptr is a live RustCommand* for the lifetime of self.
        unsafe { noesis_command_raise_can_execute_changed(self.ptr.as_ptr()) }
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_command_create with +1 ref; this
        // releases exactly that ref. The handler box is freed by the C++
        // destructor once the last reference (possibly a binding) drops.
        unsafe { noesis_command_destroy(self.ptr.as_ptr()) }
    }
}

/// A command handle: [`Command`], [`RoutedCommand`], [`RoutedUICommand`], or
/// [`BorrowedCommand`]. Accepted by [`CommandBinding::new`] and
/// [`Instance::set_command`](crate::classes::Instance::set_command).
pub trait AsCommand {
    /// Borrowed `Noesis::ICommand*` (a `BaseComponent*`), valid while `self` is.
    fn command_ptr(&self) -> *mut c_void;
}

impl AsCommand for Command {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

/// A `Noesis::RoutedCommand` created in code. It has no logic of its own:
/// executing it on an element routes up the element tree to the first
/// [`CommandBinding`] for this command. Holds one reference, released on drop.
pub struct RoutedCommand {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for RoutedCommand {}

impl RoutedCommand {
    /// Create a routed command named `name`, owned by the type named
    /// `owner_type`: a built-in such as `"UIElement"` or a class registered with
    /// [`ClassBuilder`](crate::classes::ClassBuilder). Returns `None` if no
    /// class has that name.
    ///
    /// # Panics
    ///
    /// Panics if `name` or `owner_type` contains an interior NUL byte.
    #[must_use]
    pub fn new(name: &str, owner_type: &str) -> Option<Self> {
        let cn = CString::new(name).expect("name contained interior NUL");
        let co = CString::new(owner_type).expect("owner_type contained interior NUL");
        // SAFETY: both C strings live for the call; C returns +1 or NULL.
        let ptr = unsafe { noesis_routed_command_create(cn.as_ptr(), co.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Execute the command on `target`, routing up from it to the first
    /// matching [`CommandBinding`]. Does nothing if no binding handles it. Use
    /// `CommandParameterValue::new(ptr::null_mut())` for no parameter.
    pub fn execute(&self, param: CommandParameterValue, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedCommand*; target.raw() a live element.
        unsafe {
            noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw());
        }
    }

    /// Whether the command can execute on `target`, as answered by the first
    /// matching [`CommandBinding`] up the tree. `false` if nothing handles it.
    #[must_use]
    pub fn can_execute(&self, param: CommandParameterValue, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw())
        }
    }

    /// The name the command was created with.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; returns a borrowed interned
        // string we copy immediately.
        unsafe { cstr_opt(noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Raw `Noesis::ICommand*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl AsCommand for RoutedCommand {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for RoutedCommand {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::RoutedUICommand`: a [`RoutedCommand`] with display text, e.g.
/// for menu items. Holds one reference, released on drop.
pub struct RoutedUICommand {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for RoutedUICommand {}

impl RoutedUICommand {
    /// Create a routed UI command with display label `text`. `name` and
    /// `owner_type` work as in [`RoutedCommand::new`]; returns `None` if no
    /// class is named `owner_type`.
    ///
    /// # Panics
    ///
    /// Panics if any argument contains an interior NUL byte.
    #[must_use]
    pub fn new(name: &str, text: &str, owner_type: &str) -> Option<Self> {
        let cn = CString::new(name).expect("name contained interior NUL");
        let ct = CString::new(text).expect("text contained interior NUL");
        let co = CString::new(owner_type).expect("owner_type contained interior NUL");
        // SAFETY: all C strings live for the call; C returns +1 or NULL.
        let ptr = unsafe { noesis_routed_ui_command_create(cn.as_ptr(), ct.as_ptr(), co.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// See [`RoutedCommand::execute`].
    pub fn execute(&self, param: CommandParameterValue, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedUICommand* (a RoutedCommand).
        unsafe {
            noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw());
        }
    }

    /// See [`RoutedCommand::can_execute`].
    #[must_use]
    pub fn can_execute(&self, param: CommandParameterValue, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw())
        }
    }

    /// The display text.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedUICommand*; borrowed string copied.
        unsafe { cstr_opt(noesis_routed_ui_command_get_text(self.ptr.as_ptr())) }
    }

    /// Set the display text.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL byte.
    pub fn set_text(&mut self, text: &str) {
        let c = CString::new(text).expect("text contained interior NUL");
        // SAFETY: self.ptr is a live RoutedUICommand*; c lives for the call.
        unsafe { noesis_routed_ui_command_set_text(self.ptr.as_ptr(), c.as_ptr()) };
    }

    /// The name the command was created with.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; borrowed string copied.
        unsafe { cstr_opt(noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Raw `Noesis::ICommand*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl AsCommand for RoutedUICommand {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for RoutedUICommand {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

fn param_ptr(param: &CommandParameterValue) -> *mut c_void {
    param.raw().map_or(core::ptr::null_mut(), NonNull::as_ptr)
}

/// One of the framework's built-in `RoutedUICommand`s, from
/// [`ApplicationCommand::command`] or [`ComponentCommand::command`]. The
/// framework owns these for the life of the runtime, so the handle holds no
/// reference and is `Copy`. Use it with [`CommandBinding::new`] or
/// [`Instance::set_command`](crate::classes::Instance::set_command).
#[derive(Copy, Clone)]
pub struct BorrowedCommand {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for BorrowedCommand {}

impl BorrowedCommand {
    /// Raw `Noesis::ICommand*`, valid until the runtime shuts down.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The command's display text.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedUICommand*; borrowed string copied.
        unsafe { cstr_opt(noesis_routed_ui_command_get_text(self.ptr.as_ptr())) }
    }

    /// The command's name, e.g. `"Copy"`.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; borrowed string copied.
        unsafe { cstr_opt(noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Execute on `target`. See [`RoutedCommand::execute`].
    pub fn execute(&self, param: CommandParameterValue, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedCommand*; target.raw() a live element.
        unsafe {
            noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw());
        }
    }

    /// See [`RoutedCommand::can_execute`].
    #[must_use]
    pub fn can_execute(&self, param: CommandParameterValue, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(&param), target.raw())
        }
    }
}

impl AsCommand for BorrowedCommand {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

/// The built-in `ApplicationCommands`: clipboard, document, and edit commands.
/// Get the command object with [`Self::command`].
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApplicationCommand {
    CancelPrint = 0,
    Close = 1,
    ContextMenu = 2,
    Copy = 3,
    CorrectionList = 4,
    Cut = 5,
    Delete = 6,
    Find = 7,
    Help = 8,
    New = 9,
    Open = 10,
    Paste = 11,
    Print = 12,
    PrintPreview = 13,
    Properties = 14,
    Redo = 15,
    Replace = 16,
    Save = 17,
    SaveAs = 18,
    SelectAll = 19,
    Stop = 20,
    Undo = 21,
}

impl ApplicationCommand {
    /// The framework's command object.
    ///
    /// # Panics
    ///
    /// Panics if the runtime is not initialized ([`crate::init`]).
    #[must_use]
    pub fn command(self) -> BorrowedCommand {
        // SAFETY: returns a borrowed framework singleton (valid after init()).
        let ptr = unsafe { noesis_application_command(self as u32) };
        BorrowedCommand {
            ptr: NonNull::new(ptr.cast_mut())
                .expect("ApplicationCommands singleton was null (runtime not initialized?)"),
        }
    }
}

/// The built-in `ComponentCommands`: navigation, selection, and scrolling
/// commands used inside controls. Get the command object with
/// [`Self::command`].
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComponentCommand {
    ExtendSelectionDown = 0,
    ExtendSelectionLeft = 1,
    ExtendSelectionRight = 2,
    ExtendSelectionUp = 3,
    MoveDown = 4,
    MoveFocusBack = 5,
    MoveFocusDown = 6,
    MoveFocusForward = 7,
    MoveFocusPageDown = 8,
    MoveFocusPageUp = 9,
    MoveFocusUp = 10,
    MoveLeft = 11,
    MoveRight = 12,
    MoveToEnd = 13,
    MoveToHome = 14,
    MoveToPageDown = 15,
    MoveToPageUp = 16,
    MoveUp = 17,
    ScrollByLine = 18,
    ScrollPageDown = 19,
    ScrollPageLeft = 20,
    ScrollPageRight = 21,
    ScrollPageUp = 22,
    SelectToEnd = 23,
    SelectToHome = 24,
    SelectToPageDown = 25,
    SelectToPageUp = 26,
}

impl ComponentCommand {
    /// The framework's command object.
    ///
    /// # Panics
    ///
    /// Panics if the runtime is not initialized ([`crate::init`]).
    #[must_use]
    pub fn command(self) -> BorrowedCommand {
        // SAFETY: returns a borrowed framework singleton (valid after init()).
        let ptr = unsafe { noesis_component_command(self as u32) };
        BorrowedCommand {
            ptr: NonNull::new(ptr.cast_mut())
                .expect("ComponentCommands singleton was null (runtime not initialized?)"),
        }
    }
}

/// The handlers behind a [`CommandBinding`]. Any `Fn(CommandParameterValue)`
/// closure works as a handler whose `can_execute` is always `true`.
pub trait CommandBindingHandler: Send + 'static {
    /// Whether the command can run now. Defaults to `true`.
    fn can_execute(&self, _param: CommandParameterValue) -> bool {
        true
    }

    /// Run the command's action.
    ///
    /// Takes `&self` because calls can re-enter, as with
    /// [`CommandHandler::execute`]. Use interior mutability for state.
    fn execute(&self, param: CommandParameterValue);
}

impl<F: Fn(CommandParameterValue) + Send + 'static> CommandBindingHandler for F {
    fn execute(&self, param: CommandParameterValue) {
        self(param);
    }
}

/// SAFETY: `userdata` is the double-boxed handler leaked in
/// [`CommandBinding::new`], alive until the free trampoline runs.
unsafe extern "C" fn cb_executed_trampoline(userdata: *mut c_void, param: *mut c_void) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `CommandBindingHandler`).
        let handler = &*userdata.cast::<Box<dyn CommandBindingHandler>>();
        handler.execute(CommandParameterValue::new(param));
    })
}

/// SAFETY: see [`cb_executed_trampoline`].
unsafe extern "C" fn cb_can_execute_trampoline(userdata: *mut c_void, param: *mut c_void) -> bool {
    crate::panic_guard::guard(|| {
        let handler = &*userdata.cast::<Box<dyn CommandBindingHandler>>();
        handler.can_execute(CommandParameterValue::new(param))
    })
}

/// SAFETY: matching `Box::from_raw` for the leak in [`CommandBinding::new`],
/// run exactly once by the C++ destructor.
unsafe extern "C" fn cb_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(
            userdata.cast::<Box<dyn CommandBindingHandler>>(),
        ));
    })
}

/// Handles a command for an element and its descendants.
///
/// Create it with [`Self::new`], then [`attach`](Self::attach) it to an
/// element. When the command is executed on that element or anything below
/// it, the handlers run and the routed event is marked handled, so it stops
/// there.
///
/// Dropping the binding detaches the handlers and removes it from the element.
/// Dropping it from inside its own handler is safe; teardown waits until the
/// callback returns.
pub struct CommandBinding {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CommandBinding {}

impl CommandBinding {
    /// Create a binding that handles `command` with `handler`. Returns `None`
    /// only if `command` does not point at an `ICommand`, which doesn't happen
    /// with this crate's command types.
    #[must_use]
    pub fn new<C: AsCommand, H: CommandBindingHandler>(command: &C, handler: H) -> Option<Self> {
        let boxed: Box<Box<dyn CommandBindingHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // SAFETY: trampolines are extern "C"; userdata is freshly leaked and
        // donated to the C++ bridge (freed via cb_free on destroy); the command
        // pointer is borrowed for the call only.
        let token = unsafe {
            noesis_command_binding_create(
                command.command_ptr(),
                cb_executed_trampoline,
                Some(cb_can_execute_trampoline),
                userdata.cast(),
                cb_free_trampoline,
            )
        };

        match NonNull::new(token) {
            Some(token) => Some(Self { token }),
            None => {
                // SAFETY: userdata came from Box::into_raw above; nothing took it.
                unsafe { drop(Box::from_raw(userdata)) };
                None
            }
        }
    }

    /// Add this binding to `element`'s `CommandBindings`. Returns `false` if
    /// `element` is not a `UIElement`.
    ///
    /// The binding keeps a reference to the element so drop can remove it
    /// again. Only the most recent element is remembered: if you attach to
    /// several, drop removes the binding from the last one only.
    pub fn attach(&self, element: &FrameworkElement) -> bool {
        // SAFETY: token is a live bridge; element.raw() a live element.
        unsafe { noesis_command_binding_attach(self.token.as_ptr(), element.raw()) }
    }
}

impl Drop for CommandBinding {
    fn drop(&mut self) {
        // SAFETY: token from new(); destroy detaches the delegates, removes the
        // binding from the attached element, and frees the donated handler box
        // exactly once (deferred if dropping from inside the callback).
        unsafe { noesis_command_binding_destroy(self.token.as_ptr()) }
    }
}
