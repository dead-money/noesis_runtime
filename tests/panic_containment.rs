//! Panic-safety: a user callback that `panic!`s inside an FFI trampoline must be
//! contained (turned into the trampoline's safe default) and must NOT unwind
//! across the C ABI — unwinding past `extern "C"` is undefined behavior and
//! would abort the process.
//!
//! We drive the real SDK invoker (`InvokeErrorHandler`) so the panic happens
//! *inside* `error_trampoline` exactly as Noesis would trigger it. If the
//! `catch_unwind` guard were missing, the unwind would tear the process down
//! before any later assertion could run; reaching the end of the test (and the
//! engine still being usable afterwards) is the proof of containment.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use noesis_runtime::diagnostics as diag;

#[test]
fn panicking_handler_is_contained_not_aborting() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    // Silence the default panic hook so the deliberate panics below don't spam
    // the test log with backtraces; restore it afterwards.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // Counts how many times the panicking closure was actually entered — proves
    // the trampoline really invoked it (and then swallowed the unwind).
    let entered = Arc::new(AtomicUsize::new(0));

    {
        let entered2 = Arc::clone(&entered);
        let _guard = diag::set_error_handler(move |_file, _line, _msg, _fatal| {
            entered2.fetch_add(1, Ordering::SeqCst);
            panic!("user error handler deliberately panics");
        });

        // Each of these crosses C -> Rust -> (panic) -> contained -> C. Without
        // the guard, the first call would abort the process here.
        diag::invoke_error("panic.cpp", 1, false, "boom one");
        diag::invoke_error("panic.cpp", 2, false, "boom two");
    }

    // We got here, so neither invoke aborted: the panics were contained.
    assert_eq!(
        entered.load(Ordering::SeqCst),
        2,
        "the panicking handler must have been entered for each invoke"
    );

    // The engine must still be usable after a contained panic: install a normal
    // handler and confirm it round-trips an error as usual.
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let seen2 = Arc::clone(&seen);
        let _guard = diag::set_error_handler(move |_file, _line, msg, _fatal| {
            seen2.lock().unwrap().push(msg.to_string());
        });
        diag::invoke_error("ok.cpp", 3, false, "still alive");
    }
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["still alive".to_string()],
        "engine must keep dispatching errors normally after a contained panic"
    );

    std::panic::set_hook(prev_hook);
    noesis_runtime::shutdown();
}
