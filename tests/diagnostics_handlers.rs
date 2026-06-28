//! TODO §18 — error / assert / per-thread-error handlers.
//!
//! Each handler is registered as a Rust closure that records what it saw into an
//! `Arc<Mutex<..>>`; we then drive it through the real SDK invokers
//! (`InvokeErrorHandler` / `InvokeAssertHandler`) and assert the exact values
//! crossed the FFI. The RAII guards' restore-on-drop is exercised by a nested
//! install/drop chain whose effect is observable: after a guard drops, the next
//! invoke must reach the *previous* handler (or no handler), never the dropped
//! one.
//!
//! Everything runs with `fatal = false` — a fatal error or a real assert can
//! abort the process (see `diagnostics` module docs).
//!
//! Build note: this Release Noesis dylib compiles the assert subsystem out
//! (`SetAssertHandler` / `InvokeAssertHandler` are aliased to a single no-op
//! stub), so the assert *round-trip* can't be observed here. The assert section
//! asserts whichever contract this build exposes and is strong on a Debug SDK.

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::diagnostics::{self as diag, ErrorContext};

type Rec<T> = Arc<Mutex<Vec<T>>>;

fn rec<T>() -> Rec<T> {
    Arc::new(Mutex::new(Vec::new()))
}

#[test]
fn error_assert_thread_handlers() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── Global error handler: exact value round-trip ───────────────────
        let a: Rec<(String, u32, String, bool)> = rec();
        {
            let a2 = Arc::clone(&a);
            let _ga = diag::set_error_handler(move |file, line, msg, fatal| {
                a2.lock()
                    .unwrap()
                    .push((file.into(), line, msg.into(), fatal));
            });

            diag::invoke_error("widgets.cpp", 128, false, "thing went sideways");
            {
                let got = a.lock().unwrap();
                assert_eq!(got.len(), 1, "error handler fired exactly once");
                assert_eq!(
                    got[0],
                    (
                        "widgets.cpp".to_string(),
                        128,
                        "thing went sideways".to_string(),
                        false
                    ),
                    "file/line/message/fatal must round-trip verbatim across the FFI"
                );
            }

            // ── Nested handler: takes over, leaves the outer record alone ───
            let b: Rec<(String, u32, String)> = rec();
            {
                let b2 = Arc::clone(&b);
                let _gb = diag::set_error_handler(move |file, line, msg, _fatal| {
                    b2.lock().unwrap().push((file.into(), line, msg.into()));
                });
                diag::invoke_error("inner.cpp", 7, false, "inner only");
                assert_eq!(
                    b.lock().unwrap().len(),
                    1,
                    "inner handler caught its invoke"
                );
                assert_eq!(
                    a.lock().unwrap().len(),
                    1,
                    "outer handler must NOT see the inner invoke while shadowed"
                );
                // _gb drops here → outer handler restored.
            }

            // After the nested guard dropped, the OUTER handler is active again.
            diag::invoke_error("outer.cpp", 9, false, "outer again");
            {
                let got = a.lock().unwrap();
                assert_eq!(
                    got.len(),
                    2,
                    "outer handler must catch the post-restore invoke"
                );
                assert_eq!(
                    got[1],
                    ("outer.cpp".to_string(), 9, "outer again".to_string(), false),
                );
            }
            assert_eq!(
                b.lock().unwrap().len(),
                1,
                "dropped inner handler must NOT receive later invokes"
            );
            // _ga drops here → Noesis default handler restored.
        }

        // With no custom handler, the invoke hits the Noesis default (a log) and
        // must NOT reach our dropped closure.
        diag::invoke_error("after.cpp", 1, false, "to the default");
        assert_eq!(
            a.lock().unwrap().len(),
            2,
            "no invoke may reach a handler after its guard dropped"
        );

        // ── Per-thread error handler: ErrorContext{uri,line,column} round-trip
        let t: Rec<(String, u32, String, bool, Option<ErrorContext>)> = rec();
        {
            let t2 = Arc::clone(&t);
            let _gt = diag::set_thread_error_handler(move |file, line, msg, fatal, ctx| {
                t2.lock()
                    .unwrap()
                    .push((file.into(), line, msg.into(), fatal, ctx.cloned()));
            });

            diag::invoke_error_with_context(
                "parser.cpp",
                55,
                false,
                "MainWindow.xaml",
                12,
                34,
                "unexpected token",
            );
            {
                let got = t.lock().unwrap();
                assert_eq!(got.len(), 1, "thread error handler fired once");
                let (file, line, msg, fatal, ctx) = &got[0];
                assert_eq!(file, "parser.cpp");
                assert_eq!(*line, 55);
                assert_eq!(msg, "unexpected token");
                assert!(!*fatal);
                assert_eq!(
                    ctx.as_ref(),
                    Some(&ErrorContext {
                        uri: Some("MainWindow.xaml".to_string()),
                        line: 12,
                        column: 34,
                    }),
                    "ErrorContext uri/line/column must round-trip through ErrorHandler2"
                );
            }

            // No-context invoke on the same thread handler → context is None.
            diag::invoke_error("parser.cpp", 56, false, "no ctx here");
            {
                let got = t.lock().unwrap();
                assert_eq!(got.len(), 2);
                assert_eq!(got[1].4, None, "absent context must surface as None");
            }
            // _gt drops here → per-thread handler removed.
        }

        // After the thread guard dropped, a context invoke falls through to the
        // global default and must NOT reach our dropped closure.
        diag::invoke_error_with_context("parser.cpp", 99, false, "x.xaml", 1, 1, "post-drop");
        assert_eq!(
            t.lock().unwrap().len(),
            2,
            "dropped thread handler must receive nothing further"
        );

        // ── Assert handler ─────────────────────────────────────────────────
        let s: Rec<(String, u32, String)> = rec();
        {
            let s2 = Arc::clone(&s);
            let _gs = diag::set_assert_handler(move |file, line, expr| {
                s2.lock().unwrap().push((file.into(), line, expr.into()));
                true // request a debug break
            });
            let ret = diag::invoke_assert("checks.cpp", 200, "ptr != nullptr");
            let recorded = s.lock().unwrap().clone();

            if recorded.is_empty() {
                // Release SDK (this build): the assert subsystem is compiled out
                // — SetAssertHandler/InvokeAssertHandler are a shared no-op stub,
                // so the handler never runs and the invoker returns false. Assert
                // exactly that contract so a regression that wires asserts wrong
                // (e.g. spuriously invoking) is still caught.
                assert!(
                    !ret,
                    "stubbed InvokeAssertHandler must return false on a Release SDK"
                );
            } else {
                // Debug SDK: the handler ran — assert the full round-trip and
                // that our `true` return propagated out of InvokeAssertHandler.
                assert_eq!(recorded.len(), 1, "assert handler fired exactly once");
                assert_eq!(
                    recorded[0],
                    ("checks.cpp".to_string(), 200, "ptr != nullptr".to_string()),
                    "assert file/line/expr must round-trip"
                );
                assert!(
                    ret,
                    "closure returned true → InvokeAssertHandler must return true"
                );
            }
            // _gs drops here.
        }
    }

    dm_noesis_runtime::shutdown();
}
