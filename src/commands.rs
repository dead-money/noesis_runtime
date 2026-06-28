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
use std::ffi::c_void;

use crate::ffi::{
    CommandVTable, dm_noesis_command_create, dm_noesis_command_destroy,
    dm_noesis_command_raise_can_execute_changed,
};

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
