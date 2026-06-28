//! `ICommand` from Rust (TODO §4): let XAML `Command="{Binding ...}"` invoke
//! Rust logic.
//!
//! A [`Command`] wraps a `Noesis::BaseCommand` subclass whose `CanExecute` /
//! `Execute` forward into a Rust [`CommandHandler`]. The command is a
//! `BaseComponent`, so it crosses the FFI the same way every other Rust-owned
//! Noesis value does — as an opaque pointer ([`Command::raw`]). To make it
//! reachable from XAML:
//!
//! 1. Register a Rust-backed view model with a `BaseComponent` dependency
//!    property (see [`ClassBuilder`](crate::classes::ClassBuilder)).
//! 2. Set that DP to the command:
//!    `unsafe { instance.handle().set_component(idx, command.raw()) }`.
//! 3. Expose the instance as a `DataContext`
//!    ([`FrameworkElement::set_data_context`](crate::view::FrameworkElement::set_data_context)).
//! 4. Author `<Button Command="{Binding ThatProperty}"/>` in XAML.
//!
//! When the button is clicked, Noesis calls the command's `Execute`, which
//! runs [`CommandHandler::execute`]. Noesis also queries `CanExecute` to drive
//! the button's `IsEnabled`; call [`Command::raise_can_execute_changed`] after
//! your enabled-state changes so bound controls re-query.
//!
//! # Lifetime
//!
//! [`Command`] holds the caller's `+1` reference, released on drop. If a
//! binding still references the command (the common case while a `Button` is
//! bound to it), the underlying object — and the boxed handler — stay alive
//! until that reference also drops. The handler is freed exactly once, by the
//! C++ destructor, after the last reference goes away. So a `Command` may be
//! dropped while still bound and live; `CanExecute` / `Execute` keep working.
//!
//! # Threading
//!
//! `CanExecute` / `Execute` fire from inside Noesis's input pump on whatever
//! thread drives the view. The handler is stored behind `Send`; keep the work
//! small and route to a queue if you need anything heavy.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CString, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    CommandVTable, dm_noesis_application_command, dm_noesis_base_component_release,
    dm_noesis_command_binding_attach, dm_noesis_command_binding_create,
    dm_noesis_command_binding_destroy, dm_noesis_command_create, dm_noesis_command_destroy,
    dm_noesis_command_raise_can_execute_changed, dm_noesis_component_command,
    dm_noesis_routed_command_can_execute, dm_noesis_routed_command_create,
    dm_noesis_routed_command_execute, dm_noesis_routed_command_get_name,
    dm_noesis_routed_ui_command_create, dm_noesis_routed_ui_command_get_text,
    dm_noesis_routed_ui_command_set_text,
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

/// A borrowed command parameter — Noesis's `CommandParameter` as an opaque
/// `Noesis::BaseComponent*`. `None` when the bound control supplied no
/// parameter. The pointer is borrowed for the duration of the callback; copy /
/// re-root (via Noesis accessors) if you need it past the call.
pub type CommandParameter = Option<NonNull<c_void>>;

/// Rust-side command logic. `execute` runs the action; `can_execute` gates it
/// (and drives the bound control's `IsEnabled`).
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread.
pub trait CommandHandler: Send + 'static {
    /// Whether the command can run now. Default: always `true`. Noesis calls
    /// this to decide a bound `Button`'s enabled state, and again before each
    /// `Execute`. After the answer changes, call
    /// [`Command::raise_can_execute_changed`] so bound controls re-query.
    fn can_execute(&self, _param: CommandParameter) -> bool {
        true
    }

    /// Invoke the command. Called when the bound control is activated (e.g. a
    /// `Button` click) — but only if [`Self::can_execute`] returned `true`.
    fn execute(&mut self, param: CommandParameter);
}

/// Adapter so a bare `FnMut` closure is a fire-always [`CommandHandler`]
/// (`can_execute` is always `true`). Use [`Command::new`] with a struct
/// implementing [`CommandHandler`] when you need a controllable
/// `can_execute`.
impl<F: FnMut(CommandParameter) + Send + 'static> CommandHandler for F {
    fn execute(&mut self, param: CommandParameter) {
        self(param);
    }
}

/// A single, shared vtable suffices for every command: the trampolines are
/// generic-free (they recover the `Box<dyn CommandHandler>` from `userdata`).
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
    let handler = &mut *userdata.cast::<Box<dyn CommandHandler>>();
    handler.can_execute(NonNull::new(param))
}

/// SAFETY: see [`command_can_execute_trampoline`].
unsafe extern "C" fn command_execute_trampoline(userdata: *mut c_void, param: *mut c_void) {
    let handler = &mut *userdata.cast::<Box<dyn CommandHandler>>();
    handler.execute(NonNull::new(param));
}

/// SAFETY: `userdata` was produced by [`Command::new`] and C++ owns it; this
/// is the matching `Box::from_raw` that ends that ownership, run exactly once.
unsafe extern "C" fn command_free_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    drop(Box::from_raw(userdata.cast::<Box<dyn CommandHandler>>()));
}

/// A Rust-backed `ICommand`. Owns a `+1` reference released on drop. Hand
/// [`Command::raw`] to XAML via a view-model `BaseComponent` property (see the
/// module docs).
pub struct Command {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate (per-object calls serialised by the caller).
unsafe impl Send for Command {}
unsafe impl Sync for Command {}

impl Command {
    /// Build a command from a [`CommandHandler`]. A bare
    /// `FnMut(CommandParameter)` closure also works (fire-always — its
    /// `can_execute` is always `true`).
    ///
    /// # Panics
    ///
    /// Panics only on an impossible internal invariant (`Box::into_raw`
    /// returning null / the C side returning null for a valid vtable, which it
    /// never does).
    #[must_use]
    pub fn new<H: CommandHandler>(handler: H) -> Self {
        // Double-Box for a stable thin pointer across the C ABI, matching the
        // ClickHandler / PropertyChangeHandler pattern.
        let boxed: Box<Box<dyn CommandHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // SAFETY: vtable is a 'static valid pointer; userdata is freshly
        // leaked and ownership transfers to C++; free trampoline is extern "C".
        let ptr = unsafe {
            dm_noesis_command_create(&COMMAND_VTABLE, userdata.cast(), command_free_trampoline)
        };

        match NonNull::new(ptr) {
            Some(ptr) => Command { ptr },
            None => {
                // The C side returns null only for a null vtable, which we
                // never pass — so this is unreachable in practice. Reclaim the
                // leaked box defensively rather than leak it.
                // SAFETY: userdata came from Box::into_raw above; C++ never
                // stored it (null return = nothing took ownership).
                unsafe { drop(Box::from_raw(userdata)) };
                unreachable!("dm_noesis_command_create returned null for a non-null vtable");
            }
        }
    }

    /// Raw `Noesis::BaseComponent*` (an `ICommand`), for handing to a
    /// view-model `BaseComponent` property
    /// ([`Instance::set_component`](crate::classes::Instance::set_component))
    /// or any API that takes a borrowed component. Borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Fire `CanExecuteChanged` so any control bound to this command re-queries
    /// [`CommandHandler::can_execute`] — e.g. a bound `Button` re-evaluates its
    /// `IsEnabled` on the next `View::update`. Call after your enabled-state
    /// logic changes.
    pub fn raise_can_execute_changed(&self) {
        // SAFETY: self.ptr is a live RustCommand* for the lifetime of self.
        unsafe { dm_noesis_command_raise_can_execute_changed(self.ptr.as_ptr()) }
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_command_create with +1 ref; this
        // releases exactly that ref. The handler box is freed by the C++
        // destructor once the last reference (possibly a binding) drops.
        unsafe { dm_noesis_command_destroy(self.ptr.as_ptr()) }
    }
}

// ── Routed commands (TODO §4) ────────────────────────────────────────────────

/// Anything that can be referenced as a `Noesis::ICommand*` — a [`Command`],
/// [`RoutedCommand`], [`RoutedUICommand`], or a built-in [`BorrowedCommand`].
/// Lets a [`CommandBinding`] (and any `Command` DP) accept any of them.
pub trait AsCommand {
    /// Borrowed `Noesis::ICommand*` (`BaseComponent*`), valid for `self`.
    fn command_ptr(&self) -> *mut c_void;
}

impl AsCommand for Command {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

/// A `Noesis::RoutedCommand` built in code. Unlike [`Command`] (a Rust-backed
/// `ICommand` whose logic lives in the handler), a routed command carries no
/// logic itself — invoking it routes `Execute` / `CanExecute` through the
/// element tree to the first matching [`CommandBinding`]. Owns a `+1` reference
/// released on drop.
pub struct RoutedCommand {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate.
unsafe impl Send for RoutedCommand {}
unsafe impl Sync for RoutedCommand {}

impl RoutedCommand {
    /// Create a routed command named `name`, owned by the type `owner_type`
    /// (resolved through the reflection registry — a built-in like `"UIElement"`
    /// or a [`ClassBuilder`](crate::classes)-registered custom class). Returns
    /// `None` if `owner_type` can't be resolved to a class.
    ///
    /// # Panics
    ///
    /// Panics if `name` / `owner_type` contain an interior NUL byte.
    #[must_use]
    pub fn new(name: &str, owner_type: &str) -> Option<Self> {
        let cn = CString::new(name).expect("name contained interior NUL");
        let co = CString::new(owner_type).expect("owner_type contained interior NUL");
        // SAFETY: both C strings live for the call; C returns +1 or NULL.
        let ptr = unsafe { dm_noesis_routed_command_create(cn.as_ptr(), co.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Execute the command against `target` (a `UIElement`), routing to its
    /// `CommandBinding`s. `param` is an optional borrowed command parameter.
    pub fn execute(&self, param: CommandParameter, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedCommand*; target.raw() a live element.
        unsafe {
            dm_noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(param), target.raw());
        }
    }

    /// Whether the command can currently execute against `target` (queries its
    /// `CommandBinding`s' `CanExecute`). `false` if nothing handles it.
    #[must_use]
    pub fn can_execute(&self, param: CommandParameter, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            dm_noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(param), target.raw())
        }
    }

    /// The command's registered name (`RoutedCommand::GetName`).
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; returns a borrowed interned
        // string we copy immediately.
        unsafe { cstr_opt(dm_noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Raw `Noesis::ICommand*`, borrowed for the lifetime of `self`.
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::RoutedUICommand` — a [`RoutedCommand`] plus localizable display
/// `Text` (e.g. for menu items). Owns a `+1` reference released on drop.
pub struct RoutedUICommand {
    ptr: NonNull<c_void>,
}

// SAFETY: see [`RoutedCommand`].
unsafe impl Send for RoutedUICommand {}
unsafe impl Sync for RoutedUICommand {}

impl RoutedUICommand {
    /// Create a routed UI command. `text` is the display label; see
    /// [`RoutedCommand::new`] for `name` / `owner_type`. Returns `None` if the
    /// owner type can't be resolved.
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
        let ptr =
            unsafe { dm_noesis_routed_ui_command_create(cn.as_ptr(), ct.as_ptr(), co.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// See [`RoutedCommand::execute`].
    pub fn execute(&self, param: CommandParameter, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedUICommand* (a RoutedCommand).
        unsafe {
            dm_noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(param), target.raw());
        }
    }

    /// See [`RoutedCommand::can_execute`].
    #[must_use]
    pub fn can_execute(&self, param: CommandParameter, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            dm_noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(param), target.raw())
        }
    }

    /// The display text (`RoutedUICommand::GetText`).
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedUICommand*; borrowed string copied.
        unsafe { cstr_opt(dm_noesis_routed_ui_command_get_text(self.ptr.as_ptr())) }
    }

    /// Set the display text.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL byte.
    pub fn set_text(&mut self, text: &str) {
        let c = CString::new(text).expect("text contained interior NUL");
        // SAFETY: self.ptr is a live RoutedUICommand*; c lives for the call.
        unsafe { dm_noesis_routed_ui_command_set_text(self.ptr.as_ptr(), c.as_ptr()) };
    }

    /// The command's registered name (`RoutedCommand::GetName`).
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; borrowed string copied.
        unsafe { cstr_opt(dm_noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Raw `Noesis::ICommand*`, borrowed for the lifetime of `self`.
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// `CommandParameter` → raw pointer for the C ABI (NULL when `None`).
fn param_ptr(param: CommandParameter) -> *mut c_void {
    param.map_or(core::ptr::null_mut(), NonNull::as_ptr)
}

// ── Built-in command libraries (TODO §4) ─────────────────────────────────────

/// A borrowed reference to a framework-owned `RoutedUICommand` singleton (the
/// built-in [`ApplicationCommand`] / [`ComponentCommand`] libraries). It holds
/// no reference and runs no `Drop` — the framework owns these for the process
/// lifetime — so it is `Copy`. Use it as a [`CommandBinding`] command or assign
/// it to a control's `Command` property.
#[derive(Copy, Clone)]
pub struct BorrowedCommand {
    ptr: NonNull<c_void>,
}

// SAFETY: a process-lifetime framework singleton; sharing the borrowed pointer
// across threads is sound under the same per-object-serialisation contract.
unsafe impl Send for BorrowedCommand {}
unsafe impl Sync for BorrowedCommand {}

impl BorrowedCommand {
    /// Raw `Noesis::ICommand*`, valid for the process lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The command's display text (these built-ins are `RoutedUICommand`s).
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedUICommand*; borrowed string copied.
        unsafe { cstr_opt(dm_noesis_routed_ui_command_get_text(self.ptr.as_ptr())) }
    }

    /// The command's registered name.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live RoutedCommand*; borrowed string copied.
        unsafe { cstr_opt(dm_noesis_routed_command_get_name(self.ptr.as_ptr())) }
    }

    /// Execute this command against `target` (a `UIElement`), routing to its
    /// `CommandBinding`s — the built-ins are `RoutedCommand`s. See
    /// [`RoutedCommand::execute`].
    pub fn execute(&self, param: CommandParameter, target: &FrameworkElement) {
        // SAFETY: self.ptr is a live RoutedCommand*; target.raw() a live element.
        unsafe {
            dm_noesis_routed_command_execute(self.ptr.as_ptr(), param_ptr(param), target.raw());
        }
    }

    /// Whether this command can currently execute against `target`. See
    /// [`RoutedCommand::can_execute`].
    #[must_use]
    pub fn can_execute(&self, param: CommandParameter, target: &FrameworkElement) -> bool {
        // SAFETY: as above.
        unsafe {
            dm_noesis_routed_command_can_execute(self.ptr.as_ptr(), param_ptr(param), target.raw())
        }
    }
}

impl AsCommand for BorrowedCommand {
    fn command_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

/// The `ApplicationCommands` library — common application-level commands
/// (clipboard, document, edit). [`Self::command`] returns the framework
/// singleton.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// The framework's `RoutedUICommand` singleton for this command.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis runtime is not initialized (the singletons are set
    /// up during [`crate::init`]).
    #[must_use]
    pub fn command(self) -> BorrowedCommand {
        // SAFETY: returns a borrowed framework singleton (valid after init()).
        let ptr = unsafe { dm_noesis_application_command(self as u32) };
        BorrowedCommand {
            ptr: NonNull::new(ptr.cast_mut())
                .expect("ApplicationCommands singleton was null (runtime not initialized?)"),
        }
    }
}

/// The `ComponentCommands` library — control-internal navigation / selection /
/// scrolling commands. [`Self::command`] returns the framework singleton.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// The framework's `RoutedUICommand` singleton for this command.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis runtime is not initialized.
    #[must_use]
    pub fn command(self) -> BorrowedCommand {
        // SAFETY: returns a borrowed framework singleton (valid after init()).
        let ptr = unsafe { dm_noesis_component_command(self as u32) };
        BorrowedCommand {
            ptr: NonNull::new(ptr.cast_mut())
                .expect("ComponentCommands singleton was null (runtime not initialized?)"),
        }
    }
}

// ── CommandBinding (TODO §4) ─────────────────────────────────────────────────

/// Rust handlers for a [`CommandBinding`]: `execute` runs the action when a
/// bound command is invoked through the attached element; `can_execute` gates
/// it (default always-`true`). A bare `FnMut(CommandParameter)` closure works as
/// a fire-always handler.
pub trait CommandBindingHandler: Send + 'static {
    /// Whether the command may run now. Default `true`.
    fn can_execute(&self, _param: CommandParameter) -> bool {
        true
    }

    /// Run the command's action.
    fn execute(&mut self, param: CommandParameter);
}

impl<F: FnMut(CommandParameter) + Send + 'static> CommandBindingHandler for F {
    fn execute(&mut self, param: CommandParameter) {
        self(param);
    }
}

/// SAFETY: `userdata` is the double-boxed handler leaked in
/// [`CommandBinding::new`], alive until the free trampoline runs.
unsafe extern "C" fn cb_executed_trampoline(userdata: *mut c_void, param: *mut c_void) {
    let handler = &mut *userdata.cast::<Box<dyn CommandBindingHandler>>();
    handler.execute(NonNull::new(param));
}

/// SAFETY: see [`cb_executed_trampoline`].
unsafe extern "C" fn cb_can_execute_trampoline(userdata: *mut c_void, param: *mut c_void) -> bool {
    let handler = &mut *userdata.cast::<Box<dyn CommandBindingHandler>>();
    handler.can_execute(NonNull::new(param))
}

/// SAFETY: matching `Box::from_raw` for the leak in [`CommandBinding::new`],
/// run exactly once by the C++ destructor.
unsafe extern "C" fn cb_free_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    drop(Box::from_raw(
        userdata.cast::<Box<dyn CommandBindingHandler>>(),
    ));
}

/// Binds a command to Rust handlers and (once [`attached`](Self::attach)) makes
/// an element respond to that command when it's invoked and routes through the
/// element. RAII: drop it to detach the handlers and free them.
pub struct CommandBinding {
    token: NonNull<c_void>,
}

// SAFETY: the boxed handler is `Send`; the C++ bridge is bound to one binding
// whose access Noesis serialises — mirrors the event subscriptions.
unsafe impl Send for CommandBinding {}
unsafe impl Sync for CommandBinding {}

impl CommandBinding {
    /// Build a binding for `command` (any [`AsCommand`] — a [`RoutedCommand`],
    /// [`RoutedUICommand`], built-in [`BorrowedCommand`], or [`Command`]) with
    /// the given [`CommandBindingHandler`]. Attach it to an element with
    /// [`Self::attach`]. Returns `None` only if the C entrypoint fails (e.g. a
    /// non-command pointer).
    #[must_use]
    pub fn new<C: AsCommand, H: CommandBindingHandler>(command: &C, handler: H) -> Option<Self> {
        let boxed: Box<Box<dyn CommandBindingHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        // SAFETY: trampolines are extern "C"; userdata is freshly leaked and
        // donated to the C++ bridge (freed via cb_free on destroy); the command
        // pointer is borrowed for the call only.
        let token = unsafe {
            dm_noesis_command_binding_create(
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

    /// Attach this binding to `element`'s `CommandBindings` so commands invoked
    /// on (or routing through) the element reach these handlers. Returns `false`
    /// if `element` is not a `UIElement`.
    pub fn attach(&self, element: &FrameworkElement) -> bool {
        // SAFETY: token is a live bridge; element.raw() a live element.
        unsafe { dm_noesis_command_binding_attach(self.token.as_ptr(), element.raw()) }
    }
}

impl Drop for CommandBinding {
    fn drop(&mut self) {
        // SAFETY: token from new(); destroy detaches the delegates and frees the
        // donated handler box exactly once.
        unsafe { dm_noesis_command_binding_destroy(self.token.as_ptr()) }
    }
}
